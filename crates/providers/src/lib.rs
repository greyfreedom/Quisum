#![doc = "Policy-gated model-provider adapters for Quisum."]
#![forbid(unsafe_code)]

mod http;
mod sse;
mod types;

pub use http::HttpProvider;
pub use types::{
    ConnectionTest, GenerationRequest, GenerationResponse, ModelProvider, OutputContract,
    ProviderCredential, ProviderError, ProviderEvent, ProviderEventStream, TokenUsage,
};

/// Stable identity for this workspace component.
pub const CRATE_ROLE: &str = "Policy-gated model-provider adapters for Quisum.";
