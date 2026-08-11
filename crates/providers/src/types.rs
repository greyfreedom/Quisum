use std::{fmt, pin::Pin, time::Duration};

use async_trait::async_trait;
use futures_util::Stream;
use quisum_core::{ContextPacket, ProviderProfile};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

/// Credential bytes stored only in zeroizing memory.
pub struct ProviderCredential(Zeroizing<Vec<u8>>);

impl ProviderCredential {
    /// Construct a bounded non-empty credential.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, ProviderError> {
        let bytes = Zeroizing::new(bytes.into());
        if bytes.is_empty() || bytes.len() > 16 * 1_024 || bytes.contains(&0) {
            return Err(ProviderError::InvalidConfiguration);
        }
        Ok(Self(bytes))
    }

    pub(crate) fn expose(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Debug for ProviderCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderCredential([REDACTED])")
    }
}

/// Requested provider output contract.
#[derive(Debug, Clone, PartialEq)]
pub enum OutputContract {
    /// Ordinary bounded text.
    Text,
    /// A locally revalidated JSON value matching the supplied self-contained schema.
    JsonSchema {
        /// Provider-safe schema name.
        name: String,
        /// Draft 2020-12 compatible JSON Schema.
        schema: Value,
    },
}

/// A call that can contain only an already-approved and dispatched context packet.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationRequest {
    /// Immutable outbound context packet.
    pub context: ContextPacket,
    /// Trusted method instructions, separate from quoted memory data.
    pub instructions: String,
    /// Text or structured result.
    pub output: OutputContract,
    /// Hard output-token ceiling.
    pub max_output_tokens: u32,
}

/// Provider token accounting when reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input tokens.
    pub input_tokens: Option<u64>,
    /// Output tokens.
    pub output_tokens: Option<u64>,
}

/// A complete provider result after local protocol, schema, and safety validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationResponse {
    /// Model text. For structured output this is the exact JSON text returned by the provider.
    pub text: String,
    /// Parsed and locally schema-validated JSON when requested.
    pub structured: Option<Value>,
    /// Provider-reported model identifier when available.
    pub model: Option<String>,
    /// Provider stop reason when available.
    pub finish_reason: Option<String>,
    /// Token accounting.
    pub usage: TokenUsage,
}

/// Incremental response item.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    /// User-visible text delta, released only after the complete result passes local safety checks.
    TextDelta(String),
    /// Final locally validated response.
    Completed(GenerationResponse),
}

/// Owned asynchronous event stream.
pub type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<ProviderEvent, ProviderError>> + Send + 'static>>;

/// Content-free provider connectivity result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionTest {
    /// Endpoint returned a successful status.
    pub succeeded: bool,
    /// Whole-millisecond latency without endpoint or response data.
    pub latency: Duration,
}

/// Redacted provider failure categories. URL, response bodies, prompts, and keys are omitted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// Invalid non-secret profile or request configuration.
    #[error("provider configuration is invalid")]
    InvalidConfiguration,
    /// Core egress, sensitivity, or safety policy rejected the call or result.
    #[error("provider operation was blocked by local policy")]
    PolicyBlocked,
    /// Connect or total-request deadline elapsed.
    #[error("provider request timed out")]
    Timeout,
    /// Network operation failed. Endpoint details are intentionally redacted.
    #[error("provider network request failed")]
    Network,
    /// Provider returned a non-success status.
    #[error("provider returned HTTP status {0}")]
    HttpStatus(u16),
    /// Response exceeded the local byte ceiling.
    #[error("provider response exceeded the local size limit")]
    ResponseTooLarge,
    /// Provider response did not match the expected wire protocol.
    #[error("provider response protocol was invalid")]
    InvalidProtocol,
    /// Structured output was not valid JSON or did not match its schema.
    #[error("provider structured output was invalid")]
    InvalidStructuredOutput,
}

/// Provider-neutral model interface.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Non-secret profile used by this adapter.
    fn profile(&self) -> &ProviderProfile;

    /// Generate and validate one complete response.
    async fn generate(
        &self,
        request: GenerationRequest,
    ) -> Result<GenerationResponse, ProviderError>;

    /// Start a bounded server-sent event stream.
    async fn stream(
        &self,
        request: GenerationRequest,
    ) -> Result<ProviderEventStream, ProviderError>;

    /// Test authenticated connectivity without submitting personal context.
    async fn test_connection(&self) -> Result<ConnectionTest, ProviderError>;
}
