use serde::{Deserialize, Serialize};

/// Sensitivity attached to every durable item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Eligible for relevant retrieval under ordinary policy.
    Normal,
    /// Eligible only when relevant to the current declared purpose.
    Private,
    /// Requires explicit approval before provider dispatch.
    HighlySensitive,
    /// Never crosses the provider-adapter boundary.
    LocalOnly,
}

impl Sensitivity {
    /// Whether an item may ever enter an outbound provider packet.
    #[must_use]
    pub const fn provider_eligible(self) -> bool {
        !matches!(self, Self::LocalOnly)
    }

    /// Whether provider dispatch needs explicit, packet-specific approval.
    #[must_use]
    pub const fn requires_explicit_approval(self) -> bool {
        matches!(self, Self::HighlySensitive)
    }
}
