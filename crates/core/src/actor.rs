use serde::{Deserialize, Serialize};

use crate::DomainError;

/// The identity class requesting a Core operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// The locally authorized human user.
    User,
    /// An external model host using MCP or another restricted adapter.
    ModelHost,
    /// Internal migration or maintenance logic.
    System,
    /// A schema-validated import operation.
    Import,
}

/// Fine-grained operations enforced by Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Search eligible memory with a bounded query.
    SearchMemory,
    /// Read a single bounded record.
    ReadBoundedRecord,
    /// Build a minimum context packet.
    BuildContext,
    /// Record a derived session summary.
    RecordSessionSummary,
    /// Propose an event for human review.
    ProposeEvent,
    /// Confirm or reject a proposed event.
    ReviewEvent,
    /// Propose a claim for human review.
    ProposeClaim,
    /// Propose an open question.
    ProposeOpenQuestion,
    /// Propose a behavioral experiment.
    ProposeExperiment,
    /// Propose counterevidence without applying it to a claim projection.
    AddCounterevidence,
    /// Record a direct user correction.
    RecordCorrection,
    /// Confirm, revise, defer, reject, or deprecate a claim.
    ReviewClaim,
    /// Activate, complete, or cancel a behavioral experiment.
    ReviewExperiment,
    /// Delete user data and perform dependency repair.
    DeleteData,
    /// Export the full vault.
    ExportAll,
    /// Change privacy and consent policy.
    ChangePrivacyPolicy,
    /// Change encryption or key-management settings.
    ManageEncryption,
    /// Run internal migrations and projection repair.
    MaintainVault,
}

/// An operation actor. Authorization depends on kind, not caller-supplied prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    kind: ActorKind,
}

impl Actor {
    /// Construct the authorized local-user actor.
    #[must_use]
    pub const fn user() -> Self {
        Self {
            kind: ActorKind::User,
        }
    }

    /// Construct a restricted model-host actor.
    #[must_use]
    pub const fn model_host() -> Self {
        Self {
            kind: ActorKind::ModelHost,
        }
    }

    /// Construct an internal system actor.
    #[must_use]
    pub const fn system() -> Self {
        Self {
            kind: ActorKind::System,
        }
    }

    /// Construct a validated-import actor.
    #[must_use]
    pub const fn import() -> Self {
        Self {
            kind: ActorKind::Import,
        }
    }

    /// Return the actor kind.
    #[must_use]
    pub const fn kind(self) -> ActorKind {
        self.kind
    }

    /// Determine whether this actor owns a capability.
    #[must_use]
    pub const fn allows(self, capability: Capability) -> bool {
        match self.kind {
            ActorKind::User => true,
            ActorKind::System => matches!(
                capability,
                Capability::SearchMemory
                    | Capability::ReadBoundedRecord
                    | Capability::BuildContext
                    | Capability::RecordSessionSummary
                    | Capability::MaintainVault
            ),
            ActorKind::ModelHost => matches!(
                capability,
                Capability::SearchMemory
                    | Capability::ReadBoundedRecord
                    | Capability::BuildContext
                    | Capability::RecordSessionSummary
                    | Capability::ProposeEvent
                    | Capability::ProposeClaim
                    | Capability::ProposeOpenQuestion
                    | Capability::ProposeExperiment
                    | Capability::AddCounterevidence
            ),
            ActorKind::Import => matches!(
                capability,
                Capability::ProposeEvent
                    | Capability::ProposeClaim
                    | Capability::ProposeOpenQuestion
                    | Capability::ProposeExperiment
            ),
        }
    }

    /// Return an error when a capability is not authorized.
    pub fn require(self, capability: Capability) -> Result<(), DomainError> {
        if self.allows(capability) {
            Ok(())
        } else {
            Err(DomainError::Forbidden(capability))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_host_is_proposal_only() {
        let actor = Actor::model_host();
        assert!(actor.allows(Capability::ProposeClaim));
        assert!(actor.allows(Capability::AddCounterevidence));
        assert!(!actor.allows(Capability::ReviewClaim));
        assert!(!actor.allows(Capability::DeleteData));
        assert!(!actor.allows(Capability::ExportAll));
        assert!(!actor.allows(Capability::ChangePrivacyPolicy));
        assert!(!actor.allows(Capability::ManageEncryption));
    }

    #[test]
    fn system_actor_is_maintenance_only() {
        let actor = Actor::system();
        assert!(actor.allows(Capability::MaintainVault));
        assert!(actor.allows(Capability::BuildContext));
        assert!(!actor.allows(Capability::ReviewClaim));
        assert!(!actor.allows(Capability::DeleteData));
        assert!(!actor.allows(Capability::ExportAll));
        assert!(!actor.allows(Capability::ChangePrivacyPolicy));
    }
}
