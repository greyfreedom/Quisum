use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ActorKind, DomainError, EntityId, EntityKind, Validate};

/// Content-free audit action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// Vault created.
    VaultCreated,
    /// Vault unlocked.
    VaultUnlocked,
    /// Vault locked.
    VaultLocked,
    /// Migration applied.
    MigrationApplied,
    /// Session created.
    SessionCreated,
    /// User or locally validated provider message recorded.
    MessageRecorded,
    /// Session completed or abandoned by the user.
    SessionCompleted,
    /// Concrete event recorded directly by the user.
    EventRecorded,
    /// Proposed event confirmed or rejected by the user.
    EventReviewed,
    /// Candidate proposed.
    CandidateProposed,
    /// Open question accepted, answered, or closed by the user.
    OpenQuestionReviewed,
    /// Claim recorded directly by the user as an explicit reviewed statement.
    ClaimRecorded,
    /// Claim reviewed.
    ClaimReviewed,
    /// Claim revised.
    ClaimRevised,
    /// Evidence link added.
    EvidenceAdded,
    /// Behavioral experiment activated, completed, or cancelled by the user.
    ExperimentReviewed,
    /// Direct user correction recorded.
    CorrectionRecorded,
    /// Entity deleted.
    EntityDeleted,
    /// Context preview built.
    ContextBuilt,
    /// Context approved.
    ContextApproved,
    /// Context sent to a provider.
    ContextDispatched,
    /// Export created.
    ExportCreated,
    /// Validated portable import completed.
    ImportCompleted,
    /// Backup created.
    BackupCreated,
    /// Restore completed.
    RestoreCompleted,
    /// Privacy policy changed.
    PolicyChanged,
    /// Provider connection tested.
    ProviderTested,
}

/// Actor label persisted in the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditActor {
    /// Authorized local user.
    User,
    /// Restricted model host.
    ModelHost,
    /// Internal deterministic operation.
    System,
    /// Validated import.
    Import,
}

/// Content-free affected entity class. This is derived from the typed identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEntityType {
    /// Vault.
    Vault,
    /// Reflection session.
    Session,
    /// Session message.
    Message,
    /// Session summary.
    SessionSummary,
    /// Concrete event.
    Event,
    /// Claim projection.
    Claim,
    /// Claim revision.
    Revision,
    /// Human review.
    Review,
    /// User correction.
    Correction,
    /// Open question.
    OpenQuestion,
    /// Behavioral experiment.
    Experiment,
    /// Provider context packet.
    ContextPacket,
    /// Provider profile.
    Provider,
    /// Consent record.
    Consent,
    /// Audit record.
    Audit,
    /// Formal assessment.
    Assessment,
    /// Portable export.
    Export,
    /// Encrypted backup.
    Backup,
    /// Import operation.
    Import,
}

impl From<EntityKind> for AuditEntityType {
    fn from(value: EntityKind) -> Self {
        match value {
            EntityKind::Vault => Self::Vault,
            EntityKind::Session => Self::Session,
            EntityKind::Message => Self::Message,
            EntityKind::SessionSummary => Self::SessionSummary,
            EntityKind::Event => Self::Event,
            EntityKind::Claim => Self::Claim,
            EntityKind::Revision => Self::Revision,
            EntityKind::Review => Self::Review,
            EntityKind::Correction => Self::Correction,
            EntityKind::OpenQuestion => Self::OpenQuestion,
            EntityKind::Experiment => Self::Experiment,
            EntityKind::Context => Self::ContextPacket,
            EntityKind::Provider => Self::Provider,
            EntityKind::Consent => Self::Consent,
            EntityKind::Audit => Self::Audit,
            EntityKind::Assessment => Self::Assessment,
            EntityKind::Export => Self::Export,
            EntityKind::Backup => Self::Backup,
            EntityKind::Import => Self::Import,
        }
    }
}

impl From<ActorKind> for AuditActor {
    fn from(value: ActorKind) -> Self {
        match value {
            ActorKind::User => Self::User,
            ActorKind::ModelHost => Self::ModelHost,
            ActorKind::System => Self::System,
            ActorKind::Import => Self::Import,
        }
    }
}

/// Structured audit metadata that cannot contain conversation or claim text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditDetails {
    /// No additional metadata.
    None,
    /// Migration version.
    Migration {
        /// Applied migration number.
        version: u32,
    },
    /// Lifecycle transition encoded as enum names by the caller.
    Lifecycle {
        /// Previous state name.
        from: String,
        /// New state name.
        to: String,
    },
    /// Number of affected structural records.
    AffectedRecords {
        /// Count.
        count: u64,
    },
    /// Provider connectivity result without endpoint, key, or response text.
    ProviderTest {
        /// Whether the test succeeded.
        succeeded: bool,
    },
    /// Backup or export format metadata.
    Artifact {
        /// Machine format such as jsonl or encrypted_backup.
        format: String,
        /// Record count.
        record_count: u64,
        /// Whether content is encrypted.
        encrypted: bool,
    },
}

impl AuditDetails {
    fn validate(&self) -> Result<(), DomainError> {
        match self {
            Self::Lifecycle { from, to } => {
                validate_audit_token(from)?;
                validate_audit_token(to)
            }
            Self::Artifact { format, .. } => validate_audit_token(format),
            Self::None
            | Self::Migration { .. }
            | Self::AffectedRecords { .. }
            | Self::ProviderTest { .. } => Ok(()),
        }
    }
}

fn validate_audit_token(value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(DomainError::InvalidValue("audit.details"));
    }
    Ok(())
}

/// Append-only structural audit record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Public schema version.
    pub schema_version: String,
    /// Audit identifier.
    pub id: EntityId,
    /// Authorized actor class.
    pub actor: AuditActor,
    /// Operation.
    pub action: AuditAction,
    /// Optional affected entity.
    pub entity_type: Option<AuditEntityType>,
    /// Optional affected entity identifier.
    pub entity_id: Option<EntityId>,
    /// Content-free details.
    pub details: AuditDetails,
    /// Event time.
    pub created_at: DateTime<Utc>,
}

impl AuditEvent {
    /// Construct a content-free audit record.
    #[must_use]
    pub fn new(
        actor: ActorKind,
        action: AuditAction,
        entity_id: Option<EntityId>,
        details: AuditDetails,
        created_at: DateTime<Utc>,
    ) -> Self {
        let entity_type = entity_id
            .as_ref()
            .map(|identifier| identifier.kind().into());
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Audit),
            actor: actor.into(),
            action,
            entity_type,
            entity_id,
            details,
            created_at,
        }
    }
}

impl Validate for AuditEvent {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("audit.schema_version"));
        }
        self.id.require_kind(EntityKind::Audit)?;
        if self.entity_type
            != self
                .entity_id
                .as_ref()
                .map(|identifier| identifier.kind().into())
        {
            return Err(DomainError::InvalidValue("audit.entity_type"));
        }
        self.details.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_tokens_reject_free_form_text() {
        let event = AuditEvent::new(
            ActorKind::System,
            AuditAction::ClaimRevised,
            Some(EntityId::new(EntityKind::Claim)),
            AuditDetails::Lifecycle {
                from: "confirmed".to_owned(),
                to: "This contains a private statement.".to_owned(),
            },
            Utc::now(),
        );
        assert_eq!(
            event.validate(),
            Err(DomainError::InvalidValue("audit.details"))
        );
    }

    #[test]
    fn audit_entity_type_is_derived_and_tamper_evident() {
        let mut event = AuditEvent::new(
            ActorKind::User,
            AuditAction::ClaimReviewed,
            Some(EntityId::new(EntityKind::Claim)),
            AuditDetails::None,
            Utc::now(),
        );
        assert_eq!(event.entity_type, Some(AuditEntityType::Claim));
        event.entity_type = Some(AuditEntityType::Event);
        assert_eq!(
            event.validate(),
            Err(DomainError::InvalidValue("audit.entity_type"))
        );
    }
}
