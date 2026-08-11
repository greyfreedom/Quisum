use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    DomainError, EntityId, EntityKind, Sensitivity, Source, Validate, validation::validate_text,
};

/// Lifecycle for an unresolved question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenQuestionStatus {
    /// Proposed by a model and awaiting user review.
    Proposed,
    /// Accepted as a useful unresolved question.
    Open,
    /// Answered by later evidence.
    Answered,
    /// Closed without an answer.
    Closed,
    /// Deleted.
    Deleted,
}

/// A question intentionally kept open instead of forcing a claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenQuestion {
    /// Public schema version.
    pub schema_version: String,
    /// Question identifier.
    pub id: EntityId,
    /// User-facing question.
    pub question: String,
    /// Lifecycle state.
    pub status: OpenQuestionStatus,
    /// Contexts where the question is relevant.
    pub contexts: BTreeSet<String>,
    /// Claims that led to the question.
    pub related_claims: BTreeSet<EntityId>,
    /// Sensitivity policy.
    pub sensitivity: Sensitivity,
    /// Crisis exclusion flag.
    pub crisis_related: bool,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Provenance.
    pub source: Source,
}

impl OpenQuestion {
    /// Create a user-approved open question from a reviewed claim.
    #[must_use]
    pub fn from_reviewed_claim(
        question: impl Into<String>,
        claim_id: EntityId,
        sensitivity: Sensitivity,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::OpenQuestion),
            question: question.into(),
            status: OpenQuestionStatus::Open,
            contexts: BTreeSet::new(),
            related_claims: BTreeSet::from([claim_id]),
            sensitivity,
            crisis_related: false,
            created_at: now,
            source: Source::user(),
        }
    }
}

impl Validate for OpenQuestion {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("open_question.schema_version"));
        }
        self.id.require_kind(EntityKind::OpenQuestion)?;
        validate_text(&self.question, 4_000, "open_question.question")?;
        if self.contexts.len() > 32 || self.related_claims.len() > 32 {
            return Err(DomainError::InvalidValue("open_question.links"));
        }
        for context in &self.contexts {
            validate_text(context, 80, "open_question.contexts")?;
        }
        for claim_id in &self.related_claims {
            claim_id.require_kind(EntityKind::Claim)?;
        }
        self.source.validate()?;
        Ok(())
    }
}
