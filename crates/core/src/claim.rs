use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Actor, ActorKind, Capability, DomainError, EntityId, EntityKind, Sensitivity, Source, Validate,
    validation::{validate_string_list, validate_text, validate_topic},
};

/// Epistemic class of a claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    /// A fact explicitly stated by the user.
    ExplicitFact,
    /// The user's own interpretation of themselves.
    UserSelfAssessment,
    /// A time-bounded current condition.
    CurrentState,
    /// A value the user says they choose or currently prioritize.
    Value,
    /// A supported strength.
    Strength,
    /// A need or boundary.
    NeedOrBoundary,
    /// A model- or user-proposed pattern that still needs testing.
    PatternHypothesis,
    /// A reviewed insight imported from an earlier compatible schema.
    ConfirmedInsight,
}

/// Claim lifecycle. Model-facing actors cannot enter human-review states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimStatus {
    /// Constructed but not yet placed in the review queue.
    Proposed,
    /// Awaiting human review.
    PendingReview,
    /// Accepted by the user.
    Confirmed,
    /// Edited and accepted by the user.
    Revised,
    /// The user chose to keep the claim unresolved.
    Uncertain,
    /// Rejected by the user.
    Rejected,
    /// New counterevidence or source removal undermined a confirmed claim.
    Contradicted,
    /// Replaced by a later claim.
    Superseded,
    /// Marked as no longer representative.
    Deprecated,
    /// Deleted; content must be purged by storage.
    Deleted,
}

/// Qualitative evidence description, not a probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGrade {
    /// One observation or indirect evidence.
    Limited,
    /// Multiple relevant observations without decisive conflict.
    Moderate,
    /// Recurrence across time or contexts.
    Repeated,
    /// Material counterevidence or alternatives remain.
    Contested,
}

/// The human user's review position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserEndorsement {
    /// Accepted without editing.
    Accepted,
    /// Edited and accepted.
    Edited,
    /// Deliberately left unresolved.
    Uncertain,
    /// Rejected.
    Rejected,
}

/// An evidence-backed, review-governed interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// Public schema version.
    pub schema_version: String,
    /// Claim identifier.
    pub id: EntityId,
    /// Epistemic class.
    pub kind: ClaimKind,
    /// Bounded claim wording.
    pub statement: String,
    /// Review lifecycle state.
    pub status: ClaimStatus,
    /// Contexts where the statement may apply.
    pub contexts: BTreeSet<String>,
    /// Evidence supporting the claim.
    pub supporting_evidence: BTreeSet<EntityId>,
    /// Evidence that limits or contradicts the claim.
    pub counterevidence: BTreeSet<EntityId>,
    /// Plausible explanations other than the candidate pattern.
    pub alternative_explanations: Vec<String>,
    /// Qualitative evidence grade.
    pub evidence_grade: EvidenceGrade,
    /// Explicit user review position.
    pub user_endorsement: Option<UserEndorsement>,
    /// Sensitivity policy.
    pub sensitivity: Sensitivity,
    /// Provenance.
    pub source: Source,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last projection update time.
    pub updated_at: DateTime<Utc>,
    /// Required expiry/revisit time for current-state claims.
    pub revisit_after: Option<DateTime<Utc>>,
    /// Earlier claim replaced by this claim.
    pub supersedes: Option<EntityId>,
    /// Whether the source was an acute-crisis route.
    pub crisis_related: bool,
}

impl Claim {
    /// Create a candidate that can only enter the pending review queue.
    #[must_use]
    pub fn proposal(
        kind: ClaimKind,
        statement: impl Into<String>,
        evidence_grade: EvidenceGrade,
        sensitivity: Sensitivity,
        source: Source,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Claim),
            kind,
            statement: statement.into(),
            status: ClaimStatus::PendingReview,
            contexts: BTreeSet::new(),
            supporting_evidence: BTreeSet::new(),
            counterevidence: BTreeSet::new(),
            alternative_explanations: Vec::new(),
            evidence_grade,
            user_endorsement: None,
            sensitivity,
            source,
            created_at: now,
            updated_at: now,
            revisit_after: None,
            supersedes: None,
            crisis_related: false,
        }
    }

    /// Apply a human review and return append-only review/revision records.
    pub fn apply_review(
        &mut self,
        actor: Actor,
        request: ReviewRequest,
        now: DateTime<Utc>,
    ) -> Result<ReviewOutcome, DomainError> {
        actor.require(Capability::ReviewClaim)?;
        if actor.kind() != ActorKind::User {
            return Err(DomainError::Forbidden(Capability::ReviewClaim));
        }
        self.validate()?;
        if !matches!(
            self.status,
            ClaimStatus::PendingReview | ClaimStatus::Uncertain | ClaimStatus::Contradicted
        ) {
            return Err(DomainError::InvalidClaimTransition {
                from: self.status,
                to: request.decision.target_status(),
            });
        }
        request.validate()?;
        if now < self.updated_at {
            return Err(DomainError::InvalidValue("claim.updated_at"));
        }

        let before = ClaimSnapshot::from(&*self);
        let previous_status = self.status;
        let mut updated = self.clone();
        let converted_question = match request.decision {
            ReviewDecision::Confirm => {
                updated.status = ClaimStatus::Confirmed;
                updated.user_endorsement = Some(UserEndorsement::Accepted);
                None
            }
            ReviewDecision::ReviseAndConfirm => {
                let edited = request
                    .edited_statement
                    .as_deref()
                    .ok_or(DomainError::InvalidValue("review.edited_statement"))?;
                updated.statement = edited.trim().to_owned();
                updated.status = ClaimStatus::Revised;
                updated.user_endorsement = Some(UserEndorsement::Edited);
                None
            }
            ReviewDecision::Uncertain => {
                updated.status = ClaimStatus::Uncertain;
                updated.user_endorsement = Some(UserEndorsement::Uncertain);
                None
            }
            ReviewDecision::Reject => {
                updated.status = ClaimStatus::Rejected;
                updated.user_endorsement = Some(UserEndorsement::Rejected);
                None
            }
            ReviewDecision::ConvertToOpenQuestion => {
                updated.status = ClaimStatus::Rejected;
                updated.user_endorsement = Some(UserEndorsement::Rejected);
                Some(
                    request
                        .edited_statement
                        .clone()
                        .unwrap_or_else(|| updated.statement.clone()),
                )
            }
        };
        updated.updated_at = now;
        updated.validate()?;

        let review = UserReview {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Review),
            claim_id: updated.id.clone(),
            decision: request.decision,
            edited_statement: request.edited_statement,
            note: request.note,
            reviewed_at: now,
        };
        let revision = ClaimRevision {
            id: EntityId::new(EntityKind::Revision),
            claim_id: updated.id.clone(),
            kind: ClaimRevisionKind::HumanReview,
            before,
            after: ClaimSnapshot::from(&updated),
            actor: actor.kind(),
            reason: format!(
                "human review changed claim status from {previous_status:?} to {:?}",
                updated.status
            ),
            created_at: now,
        };
        *self = updated;
        Ok(ReviewOutcome {
            review,
            revision,
            converted_question,
        })
    }

    /// Apply reviewed counterevidence. Proposal-capable model hosts cannot call this transition.
    pub fn add_counterevidence(
        &mut self,
        actor: Actor,
        evidence_id: EntityId,
        now: DateTime<Utc>,
    ) -> Result<ClaimRevision, DomainError> {
        actor.require(Capability::ReviewClaim)?;
        if actor.kind() != ActorKind::User {
            return Err(DomainError::Forbidden(Capability::ReviewClaim));
        }
        self.validate()?;
        validate_evidence_kind(&evidence_id)?;
        if now < self.updated_at {
            return Err(DomainError::InvalidValue("claim.updated_at"));
        }
        if self.supporting_evidence.contains(&evidence_id)
            || self.counterevidence.contains(&evidence_id)
        {
            return Err(DomainError::InvalidValue("claim.evidence_overlap"));
        }

        let before = ClaimSnapshot::from(&*self);
        let mut updated = self.clone();
        updated.counterevidence.insert(evidence_id);
        updated.evidence_grade = EvidenceGrade::Contested;
        if matches!(
            updated.status,
            ClaimStatus::Confirmed | ClaimStatus::Revised
        ) {
            updated.status = ClaimStatus::Contradicted;
        }
        updated.updated_at = now;
        updated.validate()?;
        let revision = ClaimRevision {
            id: EntityId::new(EntityKind::Revision),
            claim_id: updated.id.clone(),
            kind: ClaimRevisionKind::CounterevidenceAdded,
            before,
            after: ClaimSnapshot::from(&updated),
            actor: actor.kind(),
            reason: "counterevidence added".to_owned(),
            created_at: now,
        };
        *self = updated;
        Ok(revision)
    }

    /// Remove a deleted evidence source and conservatively downgrade the claim.
    pub fn remove_evidence(
        &mut self,
        actor: Actor,
        evidence_id: &EntityId,
        now: DateTime<Utc>,
    ) -> Result<Option<ClaimRevision>, DomainError> {
        actor.require(Capability::DeleteData)?;
        self.validate()?;
        if now < self.updated_at {
            return Err(DomainError::InvalidValue("claim.updated_at"));
        }
        let before = ClaimSnapshot::from(&*self);
        let mut updated = self.clone();
        let removed_support = updated.supporting_evidence.remove(evidence_id);
        let removed_counter = updated.counterevidence.remove(evidence_id);
        if !removed_support && !removed_counter {
            return Ok(None);
        }

        if removed_support {
            updated.evidence_grade = EvidenceGrade::Contested;
            if matches!(
                updated.status,
                ClaimStatus::Confirmed | ClaimStatus::Revised
            ) {
                updated.status = ClaimStatus::Contradicted;
            }
        }
        updated.updated_at = now;
        updated.validate()?;
        let revision = ClaimRevision {
            id: EntityId::new(EntityKind::Revision),
            claim_id: updated.id.clone(),
            kind: ClaimRevisionKind::EvidenceRemoved,
            before,
            after: ClaimSnapshot::from(&updated),
            actor: actor.kind(),
            reason: "source evidence deleted".to_owned(),
            created_at: now,
        };
        *self = updated;
        Ok(Some(revision))
    }

    /// Mark an accepted claim as no longer representative.
    pub fn deprecate(
        &mut self,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<ClaimRevision, DomainError> {
        actor.require(Capability::ReviewClaim)?;
        self.validate()?;
        if actor.kind() != ActorKind::User
            || !matches!(
                self.status,
                ClaimStatus::Confirmed | ClaimStatus::Revised | ClaimStatus::Contradicted
            )
        {
            return Err(DomainError::InvalidClaimTransition {
                from: self.status,
                to: ClaimStatus::Deprecated,
            });
        }
        if now < self.updated_at {
            return Err(DomainError::InvalidValue("claim.updated_at"));
        }
        let before = ClaimSnapshot::from(&*self);
        let mut updated = self.clone();
        updated.status = ClaimStatus::Deprecated;
        updated.updated_at = now;
        updated.validate()?;
        let revision = ClaimRevision {
            id: EntityId::new(EntityKind::Revision),
            claim_id: updated.id.clone(),
            kind: ClaimRevisionKind::Deprecated,
            before,
            after: ClaimSnapshot::from(&updated),
            actor: actor.kind(),
            reason: "user marked claim as no longer representative".to_owned(),
            created_at: now,
        };
        *self = updated;
        Ok(revision)
    }
}

impl Validate for Claim {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("claim.schema_version"));
        }
        self.id.require_kind(EntityKind::Claim)?;
        validate_text(&self.statement, 4_000, "claim.statement")?;
        if self.contexts.len() > 32 {
            return Err(DomainError::InvalidValue("claim.contexts"));
        }
        for context in &self.contexts {
            validate_topic(context, "claim.contexts")?;
        }
        if self.supporting_evidence.len() > 128 || self.counterevidence.len() > 128 {
            return Err(DomainError::InvalidValue("claim.evidence"));
        }
        for evidence in self
            .supporting_evidence
            .iter()
            .chain(self.counterevidence.iter())
        {
            validate_evidence_kind(evidence)?;
        }
        if !self.supporting_evidence.is_disjoint(&self.counterevidence) {
            return Err(DomainError::InvalidValue("claim.evidence_overlap"));
        }
        if !self.counterevidence.is_empty() && self.evidence_grade != EvidenceGrade::Contested {
            return Err(DomainError::InvalidValue("claim.evidence_grade"));
        }
        if matches!(
            self.evidence_grade,
            EvidenceGrade::Moderate | EvidenceGrade::Repeated
        ) && self.supporting_evidence.len() < 2
            && !matches!(
                self.status,
                ClaimStatus::Rejected
                    | ClaimStatus::Contradicted
                    | ClaimStatus::Superseded
                    | ClaimStatus::Deprecated
                    | ClaimStatus::Deleted
            )
        {
            return Err(DomainError::InvalidValue("claim.evidence_grade"));
        }
        validate_string_list(
            &self.alternative_explanations,
            12,
            1_000,
            "claim.alternative_explanations",
        )?;
        if self
            .alternative_explanations
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != self.alternative_explanations.len()
        {
            return Err(DomainError::InvalidValue("claim.alternative_explanations"));
        }
        if self.updated_at < self.created_at {
            return Err(DomainError::InvalidValue("claim.updated_at"));
        }
        if self.kind == ClaimKind::PatternHypothesis {
            if self.supporting_evidence.is_empty()
                && !matches!(
                    self.status,
                    ClaimStatus::Uncertain
                        | ClaimStatus::Rejected
                        | ClaimStatus::Contradicted
                        | ClaimStatus::Superseded
                        | ClaimStatus::Deprecated
                        | ClaimStatus::Deleted
                )
            {
                return Err(DomainError::InvalidValue(
                    "claim.pattern_supporting_evidence",
                ));
            }
            if self.alternative_explanations.is_empty() {
                return Err(DomainError::InvalidValue("claim.alternative_explanations"));
            }
        }
        if self.kind == ClaimKind::CurrentState {
            let revisit_after = self
                .revisit_after
                .ok_or(DomainError::InvalidValue("claim.revisit_after"))?;
            if revisit_after <= self.created_at {
                return Err(DomainError::InvalidValue("claim.revisit_after"));
            }
        }
        if let Some(supersedes) = &self.supersedes {
            supersedes.require_kind(EntityKind::Claim)?;
            if supersedes == &self.id {
                return Err(DomainError::InvalidValue("claim.supersedes"));
            }
        }
        match self.status {
            ClaimStatus::Proposed | ClaimStatus::PendingReview => {
                if self.user_endorsement.is_some() {
                    return Err(DomainError::InvalidValue("claim.user_endorsement"));
                }
            }
            ClaimStatus::Confirmed => {
                if !matches!(
                    self.user_endorsement,
                    Some(UserEndorsement::Accepted | UserEndorsement::Edited)
                ) {
                    return Err(DomainError::InvalidValue("claim.user_endorsement"));
                }
            }
            ClaimStatus::Revised => {
                if self.user_endorsement != Some(UserEndorsement::Edited) {
                    return Err(DomainError::InvalidValue("claim.user_endorsement"));
                }
            }
            ClaimStatus::Uncertain => {
                if self.user_endorsement != Some(UserEndorsement::Uncertain) {
                    return Err(DomainError::InvalidValue("claim.user_endorsement"));
                }
            }
            ClaimStatus::Rejected => {
                if self.user_endorsement != Some(UserEndorsement::Rejected) {
                    return Err(DomainError::InvalidValue("claim.user_endorsement"));
                }
            }
            ClaimStatus::Contradicted | ClaimStatus::Superseded | ClaimStatus::Deprecated => {
                if !matches!(
                    self.user_endorsement,
                    Some(UserEndorsement::Accepted | UserEndorsement::Edited)
                ) {
                    return Err(DomainError::InvalidValue("claim.user_endorsement"));
                }
            }
            ClaimStatus::Deleted => {}
        }
        self.source.validate()?;
        Ok(())
    }
}

fn validate_evidence_kind(id: &EntityId) -> Result<(), DomainError> {
    if matches!(
        id.kind(),
        EntityKind::Event
            | EntityKind::Message
            | EntityKind::SessionSummary
            | EntityKind::Assessment
    ) {
        Ok(())
    } else {
        Err(DomainError::InvalidValue("claim.evidence_kind"))
    }
}

/// Human decision for a pending or contradicted claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    /// Accept unchanged.
    Confirm,
    /// Edit wording and accept.
    ReviseAndConfirm,
    /// Keep unresolved.
    Uncertain,
    /// Reject the interpretation.
    Reject,
    /// Reject the claim and create an open question.
    ConvertToOpenQuestion,
}

impl ReviewDecision {
    const fn target_status(self) -> ClaimStatus {
        match self {
            Self::Confirm => ClaimStatus::Confirmed,
            Self::ReviseAndConfirm => ClaimStatus::Revised,
            Self::Uncertain => ClaimStatus::Uncertain,
            Self::Reject | Self::ConvertToOpenQuestion => ClaimStatus::Rejected,
        }
    }
}

/// Input supplied by an authorized human review surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequest {
    /// Review choice.
    pub decision: ReviewDecision,
    /// Required for revised wording and optional as an open-question rewrite.
    pub edited_statement: Option<String>,
    /// Optional private note.
    pub note: Option<String>,
}

impl Validate for ReviewRequest {
    fn validate(&self) -> Result<(), DomainError> {
        match self.decision {
            ReviewDecision::ReviseAndConfirm => validate_text(
                self.edited_statement
                    .as_deref()
                    .ok_or(DomainError::InvalidValue("review.edited_statement"))?,
                4_000,
                "review.edited_statement",
            )?,
            ReviewDecision::ConvertToOpenQuestion => {
                if let Some(statement) = &self.edited_statement {
                    validate_text(statement, 4_000, "review.edited_statement")?;
                }
            }
            ReviewDecision::Confirm | ReviewDecision::Uncertain | ReviewDecision::Reject => {
                if self.edited_statement.is_some() {
                    return Err(DomainError::InvalidValue("review.edited_statement"));
                }
            }
        }
        if let Some(note) = &self.note {
            if note.chars().count() > 4_000 {
                return Err(DomainError::InvalidValue("review.note"));
            }
        }
        Ok(())
    }
}

/// Immutable human review record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserReview {
    /// Public schema version.
    pub schema_version: String,
    /// Review identifier.
    pub id: EntityId,
    /// Reviewed claim.
    pub claim_id: EntityId,
    /// Human decision.
    pub decision: ReviewDecision,
    /// Edited wording when supplied.
    pub edited_statement: Option<String>,
    /// Optional note.
    pub note: Option<String>,
    /// Review time.
    pub reviewed_at: DateTime<Utc>,
}

impl Validate for UserReview {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("review.schema_version"));
        }
        self.id.require_kind(EntityKind::Review)?;
        self.claim_id.require_kind(EntityKind::Claim)?;
        ReviewRequest {
            decision: self.decision,
            edited_statement: self.edited_statement.clone(),
            note: self.note.clone(),
        }
        .validate()
    }
}

/// Reason for a claim revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimRevisionKind {
    /// A human review changed the projection.
    HumanReview,
    /// Counterevidence was appended.
    CounterevidenceAdded,
    /// A source was deleted.
    EvidenceRemoved,
    /// The user marked the claim no longer representative.
    Deprecated,
    /// A later claim replaced this one.
    Superseded,
}

/// Immutable claim state stored before and after a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSnapshot {
    /// Wording at this revision.
    pub statement: String,
    /// Lifecycle state.
    pub status: ClaimStatus,
    /// Evidence grade.
    pub evidence_grade: EvidenceGrade,
    /// Human position.
    pub user_endorsement: Option<UserEndorsement>,
    /// Supporting evidence.
    pub supporting_evidence: BTreeSet<EntityId>,
    /// Counterevidence.
    pub counterevidence: BTreeSet<EntityId>,
    /// Applicable contexts.
    pub contexts: BTreeSet<String>,
}

impl From<&Claim> for ClaimSnapshot {
    fn from(claim: &Claim) -> Self {
        Self {
            statement: claim.statement.clone(),
            status: claim.status,
            evidence_grade: claim.evidence_grade,
            user_endorsement: claim.user_endorsement,
            supporting_evidence: claim.supporting_evidence.clone(),
            counterevidence: claim.counterevidence.clone(),
            contexts: claim.contexts.clone(),
        }
    }
}

/// Append-only explanation of a projection change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRevision {
    /// Revision identifier.
    pub id: EntityId,
    /// Affected claim.
    pub claim_id: EntityId,
    /// Revision category.
    pub kind: ClaimRevisionKind,
    /// Projection before the change.
    pub before: ClaimSnapshot,
    /// Projection after the change.
    pub after: ClaimSnapshot,
    /// Authorized actor class.
    pub actor: ActorKind,
    /// Content-free structural reason.
    pub reason: String,
    /// Revision time.
    pub created_at: DateTime<Utc>,
}

impl Validate for ClaimRevision {
    fn validate(&self) -> Result<(), DomainError> {
        self.id.require_kind(EntityKind::Revision)?;
        self.claim_id.require_kind(EntityKind::Claim)?;
        validate_snapshot(&self.before)?;
        validate_snapshot(&self.after)?;
        if !matches!(self.actor, ActorKind::User | ActorKind::System)
            || self.reason.trim().is_empty()
            || self.reason.chars().count() > 500
            || self.reason.contains(['\r', '\n'])
        {
            return Err(DomainError::InvalidValue("revision.metadata"));
        }
        Ok(())
    }
}

fn validate_snapshot(snapshot: &ClaimSnapshot) -> Result<(), DomainError> {
    validate_text(&snapshot.statement, 4_000, "revision.statement")?;
    if snapshot.supporting_evidence.len() > 128
        || snapshot.counterevidence.len() > 128
        || snapshot.contexts.len() > 32
    {
        return Err(DomainError::InvalidValue("revision.links"));
    }
    for evidence in snapshot
        .supporting_evidence
        .iter()
        .chain(snapshot.counterevidence.iter())
    {
        validate_evidence_kind(evidence)?;
    }
    for context in &snapshot.contexts {
        validate_text(context, 80, "revision.contexts")?;
    }
    Ok(())
}

/// Complete result of applying a human review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewOutcome {
    /// Immutable review record.
    pub review: UserReview,
    /// Immutable claim revision.
    pub revision: ClaimRevision,
    /// Optional question text created by conversion.
    pub converted_question: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    fn pattern_claim() -> Claim {
        let mut claim = Claim::proposal(
            ClaimKind::PatternHypothesis,
            "Evaluation authority may contribute to delayed disagreement.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            Source::user(),
            Utc::now(),
        );
        claim
            .supporting_evidence
            .insert(EntityId::new(EntityKind::Event));
        claim
            .alternative_explanations
            .push("The underlying facts may have been unclear.".to_owned());
        claim
    }

    #[test]
    fn pattern_requires_support_and_an_alternative() {
        let claim = Claim::proposal(
            ClaimKind::PatternHypothesis,
            "A tentative pattern.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            Source::user(),
            Utc::now(),
        );
        assert_eq!(
            claim.validate(),
            Err(DomainError::InvalidValue(
                "claim.pattern_supporting_evidence"
            ))
        );
    }

    #[test]
    fn model_host_cannot_confirm_a_claim() {
        let mut claim = pattern_claim();
        let result = claim.apply_review(
            Actor::model_host(),
            ReviewRequest {
                decision: ReviewDecision::Confirm,
                edited_statement: None,
                note: None,
            },
            Utc::now(),
        );
        assert_eq!(result, Err(DomainError::Forbidden(Capability::ReviewClaim)));
    }

    #[test]
    fn user_review_is_append_only_and_updates_projection() -> Result<(), Box<dyn Error>> {
        let mut claim = pattern_claim();
        let original = claim.clone();
        let outcome = claim.apply_review(
            Actor::user(),
            ReviewRequest {
                decision: ReviewDecision::ReviseAndConfirm,
                edited_statement: Some(
                    "At work, evaluation authority sometimes contributes to delayed disagreement."
                        .to_owned(),
                ),
                note: Some("The narrower scope fits.".to_owned()),
            },
            Utc::now(),
        )?;

        assert_eq!(claim.status, ClaimStatus::Revised);
        assert_eq!(claim.user_endorsement, Some(UserEndorsement::Edited));
        assert_eq!(outcome.revision.before.statement, original.statement);
        assert_eq!(outcome.revision.after.statement, claim.statement);
        Ok(())
    }

    #[test]
    fn counterevidence_flags_a_confirmed_claim_for_review() -> Result<(), Box<dyn Error>> {
        let mut claim = pattern_claim();
        claim.apply_review(
            Actor::user(),
            ReviewRequest {
                decision: ReviewDecision::Confirm,
                edited_statement: None,
                note: None,
            },
            Utc::now(),
        )?;
        let evidence_id = EntityId::new(EntityKind::Event);
        assert_eq!(
            claim.add_counterevidence(Actor::model_host(), evidence_id.clone(), Utc::now()),
            Err(DomainError::Forbidden(Capability::ReviewClaim))
        );
        claim.add_counterevidence(Actor::user(), evidence_id, Utc::now())?;
        assert_eq!(claim.status, ClaimStatus::Contradicted);
        assert_eq!(claim.evidence_grade, EvidenceGrade::Contested);
        Ok(())
    }

    #[test]
    fn failed_transition_does_not_partially_mutate_a_claim() -> Result<(), Box<dyn Error>> {
        let mut claim = pattern_claim();
        let before = claim.clone();
        let evidence_id = claim
            .supporting_evidence
            .iter()
            .next()
            .cloned()
            .ok_or("synthetic claim has no evidence")?;
        assert!(
            claim
                .remove_evidence(Actor::user(), &evidence_id, Utc::now())
                .is_err()
        );
        assert_eq!(claim, before);
        Ok(())
    }
}
