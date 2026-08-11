use std::collections::VecDeque;

use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use serde_json::Value;

use crate::{ProviderError, types::ProviderEvent};

const MAX_STREAM_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_EVENT_BYTES: usize = 1_024 * 1_024;

pub(crate) struct SseState {
    body: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static>>,
    buffer: Vec<u8>,
    pending: VecDeque<ProviderEvent>,
    accumulated: String,
    total_bytes: usize,
    stream_kind: StreamKind,
    stopped: bool,
    body_ended: bool,
    terminal_success: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StreamKind {
    OpenaiResponses,
    AnthropicMessages,
    OpenaiChat,
}

impl SseState {
    pub(crate) fn new(
        body: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        stream_kind: StreamKind,
    ) -> Self {
        Self {
            body: Box::pin(body),
            buffer: Vec::new(),
            pending: VecDeque::new(),
            accumulated: String::new(),
            total_bytes: 0,
            stream_kind,
            stopped: false,
            body_ended: false,
            terminal_success: false,
        }
    }

    pub(crate) async fn next(&mut self) -> Result<Option<ProviderEvent>, ProviderError> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            if self.stopped {
                return Ok(None);
            }
            if self.body_ended {
                return Err(ProviderError::InvalidProtocol);
            }
            while let Some(event_bytes) = take_event(&mut self.buffer)? {
                if let Some(data) = event_data(&event_bytes)? {
                    self.consume_data(&data)?;
                }
                if let Some(event) = self.pending.pop_front() {
                    return Ok(Some(event));
                }
                if self.stopped {
                    return Ok(None);
                }
            }
            match self.body.next().await {
                Some(Ok(chunk)) => {
                    self.total_bytes = self
                        .total_bytes
                        .checked_add(chunk.len())
                        .ok_or(ProviderError::ResponseTooLarge)?;
                    if self.total_bytes > MAX_STREAM_BYTES
                        || self.buffer.len().saturating_add(chunk.len()) > MAX_EVENT_BYTES
                    {
                        return Err(ProviderError::ResponseTooLarge);
                    }
                    self.buffer.extend_from_slice(&chunk);
                }
                Some(Err(error)) => return Err(map_reqwest(error)),
                None => {
                    if !self.buffer.iter().all(u8::is_ascii_whitespace) {
                        let remaining = std::mem::take(&mut self.buffer);
                        if let Some(data) = event_data(&remaining)? {
                            self.consume_data(&data)?;
                        }
                    }
                    self.body_ended = true;
                    if let Some(event) = self.pending.pop_front() {
                        return Ok(Some(event));
                    }
                    if self.stopped {
                        return Ok(None);
                    }
                    return Err(ProviderError::InvalidProtocol);
                }
            }
        }
    }

    pub(crate) fn accumulated(&self) -> &str {
        &self.accumulated
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped
    }

    fn consume_data(&mut self, data: &str) -> Result<(), ProviderError> {
        if data.trim() == "[DONE]" {
            if matches!(self.stream_kind, StreamKind::OpenaiChat) && self.terminal_success {
                self.stopped = true;
                return Ok(());
            }
            return Err(ProviderError::InvalidProtocol);
        }
        let value: Value =
            serde_json::from_str(data).map_err(|_| ProviderError::InvalidProtocol)?;
        let event_type = value.get("type").and_then(Value::as_str);
        if matches!(
            event_type,
            Some("error" | "response.failed" | "response.incomplete")
        ) {
            return Err(ProviderError::InvalidProtocol);
        }
        let delta = match self.stream_kind {
            StreamKind::OpenaiResponses if event_type == Some("response.output_text.delta") => {
                value.get("delta").and_then(Value::as_str)
            }
            StreamKind::AnthropicMessages
                if event_type == Some("content_block_delta")
                    && value.pointer("/delta/type").and_then(Value::as_str)
                        == Some("text_delta") =>
            {
                value.pointer("/delta/text").and_then(Value::as_str)
            }
            StreamKind::OpenaiChat => value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str),
            StreamKind::OpenaiResponses | StreamKind::AnthropicMessages => None,
        };
        if let Some(delta) = delta {
            if self.accumulated.len().saturating_add(delta.len()) > MAX_STREAM_BYTES {
                return Err(ProviderError::ResponseTooLarge);
            }
            self.accumulated.push_str(delta);
            self.pending
                .push_back(ProviderEvent::TextDelta(delta.to_owned()));
        }
        match self.stream_kind {
            StreamKind::OpenaiResponses if event_type == Some("response.completed") => {
                if value.pointer("/response/status").and_then(Value::as_str) != Some("completed")
                    || value
                        .pointer("/response/error")
                        .is_some_and(|error| !error.is_null())
                {
                    return Err(ProviderError::InvalidProtocol);
                }
                self.terminal_success = true;
                self.stopped = true;
            }
            StreamKind::AnthropicMessages if event_type == Some("message_delta") => {
                match value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    Some("end_turn" | "stop_sequence" | "refusal") => {
                        self.terminal_success = true;
                    }
                    Some(_) => return Err(ProviderError::InvalidProtocol),
                    None => {}
                }
            }
            StreamKind::AnthropicMessages if event_type == Some("message_stop") => {
                if !self.terminal_success {
                    return Err(ProviderError::InvalidProtocol);
                }
                self.stopped = true;
            }
            StreamKind::OpenaiChat => {
                if let Some(reason) = value
                    .pointer("/choices/0/finish_reason")
                    .and_then(Value::as_str)
                {
                    if reason != "stop" {
                        return Err(ProviderError::InvalidProtocol);
                    }
                    self.terminal_success = true;
                }
            }
            StreamKind::OpenaiResponses | StreamKind::AnthropicMessages => {}
        }
        Ok(())
    }
}

fn take_event(buffer: &mut Vec<u8>) -> Result<Option<Vec<u8>>, ProviderError> {
    let position = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|position| (position, 2))
        .or_else(|| {
            buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| (position, 4))
        });
    let Some((position, delimiter)) = position else {
        return Ok(None);
    };
    if position > MAX_EVENT_BYTES {
        return Err(ProviderError::ResponseTooLarge);
    }
    let event = buffer[..position].to_vec();
    buffer.drain(..position + delimiter);
    Ok(Some(event))
}

fn event_data(bytes: &[u8]) -> Result<Option<String>, ProviderError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ProviderError::InvalidProtocol)?;
    let mut data = String::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    Ok((!data.is_empty()).then_some(data))
}

pub(crate) fn map_reqwest(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Network
    }
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    #[tokio::test]
    async fn parser_handles_fragmented_crlf_events_and_unknown_types() -> Result<(), ProviderError>
    {
        let chunks = vec![
            Ok(Bytes::from_static(b"event: ping\r\ndata: {\"type\":\"ping\"}\r\n\r")),
            Ok(Bytes::from_static(b"\nevent: delta\r\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\r\n\r\n")),
            Ok(Bytes::from_static(b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n")),
        ];
        let mut state = SseState::new(stream::iter(chunks), StreamKind::AnthropicMessages);
        assert_eq!(
            state.next().await?,
            Some(ProviderEvent::TextDelta("hel".to_owned()))
        );
        assert_eq!(
            state.next().await?,
            Some(ProviderEvent::TextDelta("lo".to_owned()))
        );
        assert_eq!(state.next().await?, None);
        assert_eq!(state.accumulated(), "hello");
        Ok(())
    }

    #[tokio::test]
    async fn completed_openai_stream_cannot_carry_an_error() {
        let chunks = vec![Ok(Bytes::from_static(
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"error\":{\"code\":\"synthetic\"}}}\n\n",
        ))];
        let mut state = SseState::new(stream::iter(chunks), StreamKind::OpenaiResponses);
        assert_eq!(
            state.next().await,
            Ok(Some(ProviderEvent::TextDelta("partial".to_owned())))
        );
        assert_eq!(state.next().await, Err(ProviderError::InvalidProtocol));
    }
}
