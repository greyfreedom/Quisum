use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Actor, ActorKind, Capability, DomainError, EntityId, EntityKind, Sensitivity, Source, Validate,
    validation::validate_text,
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
    /// Create an unresolved candidate. Model-originated candidates remain proposed until review.
    #[must_use]
    pub fn proposal(
        question: impl Into<String>,
        sensitivity: Sensitivity,
        source: Source,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::OpenQuestion),
            question: question.into(),
            status: OpenQuestionStatus::Proposed,
            contexts: BTreeSet::new(),
            related_claims: BTreeSet::new(),
            sensitivity,
            crisis_related: false,
            created_at: now,
            source,
        }
    }

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

    /// Apply a human lifecycle decision without turning the question into a claim.
    pub fn transition(
        &mut self,
        actor: Actor,
        target: OpenQuestionStatus,
    ) -> Result<(), DomainError> {
        actor.require(Capability::ReviewOpenQuestion)?;
        if actor.kind() != ActorKind::User {
            return Err(DomainError::Forbidden(Capability::ReviewOpenQuestion));
        }
        let allowed = matches!(
            (self.status, target),
            (OpenQuestionStatus::Proposed, OpenQuestionStatus::Open)
                | (OpenQuestionStatus::Proposed, OpenQuestionStatus::Closed)
                | (OpenQuestionStatus::Open, OpenQuestionStatus::Answered)
                | (OpenQuestionStatus::Open, OpenQuestionStatus::Closed)
        );
        if !allowed {
            return Err(DomainError::InvalidValue("open_question.transition"));
        }
        self.validate()?;
        let mut updated = self.clone();
        updated.status = target;
        updated.validate()?;
        *self = updated;
        Ok(())
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

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn only_the_user_can_advance_the_open_question_lifecycle() {
        let mut question = OpenQuestion::proposal(
            "Could this pattern depend on the current context?",
            Sensitivity::Private,
            Source::model(
                "pattern-validation@1.0.0",
                "synthetic-provider",
                "synthetic-model",
                EntityId::new(EntityKind::Context),
            ),
            Utc::now(),
        );

        assert!(matches!(
            question.transition(Actor::model_host(), OpenQuestionStatus::Open),
            Err(DomainError::Forbidden(Capability::ReviewOpenQuestion))
        ));
        assert_eq!(question.status, OpenQuestionStatus::Proposed);

        assert!(
            question
                .transition(Actor::user(), OpenQuestionStatus::Open)
                .is_ok()
        );
        assert!(
            question
                .transition(Actor::user(), OpenQuestionStatus::Answered)
                .is_ok()
        );
        assert_eq!(question.status, OpenQuestionStatus::Answered);
        assert!(
            question
                .transition(Actor::user(), OpenQuestionStatus::Open)
                .is_err()
        );
    }
}
