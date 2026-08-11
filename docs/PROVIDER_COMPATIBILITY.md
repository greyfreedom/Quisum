# Provider compatibility

Compatibility claims are scoped to an adapter protocol, exact endpoint/model, declared capability class, prompt bundle, and test date. “Implemented” means the wire shape and local policy path exist. “Fixture verified” means deterministic synthetic loopback tests passed. Neither means a live service or model is certified.

## V1 adapter matrix

| Profile kind | Generation protocol | Streaming parser | Structured output | Synthetic fixture status | Accepted live-run evidence |
| --- | --- | --- | --- | --- | --- |
| OpenAI | Responses API at `/responses` | Responses SSE terminal-state checks | Provider request plus strict local JSON Schema revalidation | Implemented and fixture verified | None |
| Anthropic | Messages API at `/messages` | Messages SSE terminal-state checks | Provider request plus strict local JSON Schema revalidation | Implemented and fixture verified | None |
| OpenAI-compatible | Chat Completions at `/chat/completions` | OpenAI-compatible SSE checks | Endpoint-specific request plus strict local revalidation | Implemented and fixture verified | None |
| Local HTTP | Loopback-only Chat Completions | OpenAI-compatible SSE checks | Endpoint-specific request plus strict local revalidation | Implemented and fixture verified | None |

The connection test uses `GET /models`, sends no personal context, and records only success and latency. Provider generation uses a bounded request/response size, connect deadline, total deadline, disabled redirects, disabled ambient proxy lookup, redacted errors, and post-response safety checks. Streaming deltas are buffered until the complete terminal response passes local safety validation; unsafe or truncated streams release no partial text.

## Capability classes

- `text`: bounded ordinary response.
- `streaming`: SSE support for the profile's protocol with a successful terminal event.
- `json_schema`: the endpoint accepts the adapter's structured-output request shape; Quisum still parses and validates locally.
- `tool_use` and `vision`: recorded for forward compatibility and method selection, but V1 does not expose provider tools, images, filesystem access, or human-review authority through generation.

A declared context limit bounds the packet but does not guarantee a provider's tokenizer or quota behavior. Model aliases can change behind the same string. Retention and training claims remain the provider's responsibility.

## Deterministic protocol evidence

Run the complete local suite:

```sh
cargo test --locked -p quisum-providers --all-features
```

It covers authenticated header redaction, endpoint construction, complete responses, all three SSE dialects, fragmented CRLF events, oversized/truncated/error states, no early delta release, dispatched-context enforcement, quoted memory, and local schema revalidation.

## Bounded model-evaluation evidence

The offline policy projection is deterministic and does not call a model:

```sh
cargo run --locked -p quisum-evals --bin quisum-eval -- --json
```

To evaluate real provider output, first produce one JSONL result for every exact repository case using the same adapter, model, capability class, prompt-bundle version, and timestamp. Then validate it locally:

```sh
cargo run --locked -p quisum-evals --bin quisum-eval -- \
  --provider-results /private/path/provider-results.jsonl \
  --json
```

The runner rejects missing, duplicate, extra, mixed-run, malformed, or weakly typed results and fails critical assertions. Result files may contain sensitive prompts or outputs; keep them outside the repository and sanitize any evidence summary. A passing run applies only to that exact tuple and time.

## Current conclusion

The V1.0 RC has complete deterministic adapter and policy tests. No complete live-provider matrix has been accepted, so the `live-provider-compatibility` stable gate remains open in [release readiness](../release/readiness.json).
