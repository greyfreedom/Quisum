use std::{cmp::Reverse, collections::BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Actor, ActorKind, Capability, DomainError, EntityId, EntityKind, Sensitivity, Validate,
    validation::{validate_method_id, validate_text},
};

/// Domain item kinds eligible for bounded context retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemType {
    /// Recent user or assistant message.
    Message,
    /// Compressed session history.
    SessionSummary,
    /// Concrete event.
    Event,
    /// Reviewed claim.
    Claim,
    /// Direct user correction.
    Correction,
    /// Open question.
    OpenQuestion,
    /// Active behavioral experiment.
    Experiment,
}

/// Retrieval state independent of individual domain lifecycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryState {
    /// Directly authored or reviewed and eligible.
    Confirmed,
    /// Active correction, question, or experiment.
    Active,
    /// Awaiting human review.
    PendingReview,
    /// Explicitly rejected.
    Rejected,
    /// Deprecated or superseded.
    Deprecated,
    /// Deleted.
    Deleted,
    /// Time-bounded state past its revisit time.
    Expired,
}

/// A ranked retrieval candidate supplied by a local search implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCandidate {
    /// Item type.
    pub item_type: ContextItemType,
    /// Stable identifier.
    pub item_id: EntityId,
    /// Bounded excerpt.
    pub excerpt: String,
    /// Sensitivity policy.
    pub sensitivity: Sensitivity,
    /// Current retrieval state.
    pub state: MemoryState,
    /// Whether it came from an acute crisis route.
    pub crisis_related: bool,
    /// Whether local retrieval determined it relevant to the declared purpose.
    pub relevant: bool,
    /// Content-free explanation for relevance.
    pub selection_reason: String,
    /// Higher values rank first after correction priority.
    pub relevance_score: u16,
    /// Used as a deterministic recency tie-breaker.
    pub recorded_at: DateTime<Utc>,
}

/// Why a candidate was excluded from a provider context packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// Never provider eligible.
    LocalOnly,
    /// Retrieval found no meaningful relation to purpose.
    NotRelevant,
    /// Candidate is not reviewed.
    PendingReview,
    /// User rejected it.
    Rejected,
    /// User deleted it.
    Deleted,
    /// Acute-crisis content is not identity memory.
    CrisisRelated,
    /// A current state expired.
    Expired,
    /// Privacy topic policy denied it.
    TopicPolicy,
    /// User approval is needed before selection.
    RequiresExplicitApproval,
    /// Minimum-context size limit was reached.
    ResultLimit,
    /// Claim was deprecated or superseded.
    Deprecated,
}

/// Included context item. Text remains untrusted data, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    /// Item type.
    pub item_type: ContextItemType,
    /// Stable identifier.
    pub item_id: EntityId,
    /// Bounded untrusted excerpt.
    pub excerpt: String,
    /// Provider-eligible sensitivity.
    pub sensitivity: Sensitivity,
    /// Content-free selection reason.
    pub selection_reason: String,
}

/// Auditable exclusion without copying excluded private text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextExclusion {
    /// Excluded item identifier.
    pub item_id: EntityId,
    /// Policy reason.
    pub reason: ExclusionReason,
}

/// Immutable minimum-context selection plus approval/dispatch state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacket {
    /// Public schema version.
    pub schema_version: String,
    /// Packet identifier.
    pub id: EntityId,
    /// Declared provider-call purpose.
    pub purpose: String,
    /// Active method and version.
    pub method: String,
    /// Provider profile selected by the user.
    pub provider_profile_id: EntityId,
    /// Model identifier.
    pub model: String,
    /// Current user request.
    pub query: String,
    /// Included untrusted records.
    pub items: Vec<ContextItem>,
    /// Excluded record IDs and reasons.
    pub exclusions: Vec<ContextExclusion>,
    /// Whether included highly sensitive content needs approval.
    pub requires_approval: bool,
    /// Human approval time.
    pub approved_at: Option<DateTime<Utc>>,
    /// Build time.
    pub created_at: DateTime<Utc>,
    /// Provider-dispatch time.
    pub dispatched_at: Option<DateTime<Utc>>,
}

impl ContextPacket {
    /// Approve one packet. Approval does not grant a reusable topic permission.
    pub fn approve(&mut self, actor: Actor, approved_at: DateTime<Utc>) -> Result<(), DomainError> {
        actor.require(Capability::ChangePrivacyPolicy)?;
        if actor.kind() != ActorKind::User {
            return Err(DomainError::Forbidden(Capability::ChangePrivacyPolicy));
        }
        if !self.requires_approval || self.approved_at.is_some() || approved_at < self.created_at {
            return Err(DomainError::InvalidValue("context.approved_at"));
        }
        self.approved_at = Some(approved_at);
        self.validate()
    }

    /// Mark a packet dispatched only after a final policy validation.
    pub fn mark_dispatched(
        &mut self,
        actor: Actor,
        dispatched_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        actor.require(Capability::BuildContext)?;
        if !matches!(actor.kind(), ActorKind::User | ActorKind::System) {
            return Err(DomainError::Forbidden(Capability::BuildContext));
        }
        self.authorize_egress()?;
        if self.dispatched_at.is_some()
            || dispatched_at < self.created_at
            || self
                .approved_at
                .is_some_and(|approved| dispatched_at < approved)
        {
            return Err(DomainError::InvalidValue("context.dispatched_at"));
        }
        self.dispatched_at = Some(dispatched_at);
        self.validate()
    }

    /// Perform the last deterministic egress check before provider I/O.
    pub fn authorize_egress(&self) -> Result<(), DomainError> {
        self.validate()?;
        if self
            .items
            .iter()
            .any(|item| !item.sensitivity.provider_eligible())
        {
            return Err(DomainError::InvalidValue("context.local_only"));
        }
        if self.requires_approval && self.approved_at.is_none() {
            return Err(DomainError::InvalidValue("context.approved_at"));
        }
        Ok(())
    }
}

impl Validate for ContextPacket {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("context.schema_version"));
        }
        self.id.require_kind(EntityKind::Context)?;
        self.provider_profile_id
            .require_kind(EntityKind::Provider)?;
        validate_text(&self.purpose, 240, "context.purpose")?;
        validate_method_id(&self.method, "context.method")?;
        validate_text(&self.model, 160, "context.model")?;
        validate_text(&self.query, 4_000, "context.query")?;
        if self.items.len() > 64 || self.exclusions.len() > 128 {
            return Err(DomainError::InvalidValue("context.item_limit"));
        }
        let has_highly_sensitive = self
            .items
            .iter()
            .any(|item| item.sensitivity == Sensitivity::HighlySensitive);
        if has_highly_sensitive != self.requires_approval {
            return Err(DomainError::InvalidValue("context.requires_approval"));
        }
        let mut included_ids = BTreeSet::new();
        for item in &self.items {
            if !item.sensitivity.provider_eligible() {
                return Err(DomainError::InvalidValue("context.local_only"));
            }
            item.item_id.require_kind(entity_kind(item.item_type))?;
            if !included_ids.insert(item.item_id.clone()) {
                return Err(DomainError::InvalidValue("context.duplicate_item"));
            }
            validate_text(&item.excerpt, 8_000, "context.excerpt")?;
            validate_text(&item.selection_reason, 500, "context.selection_reason")?;
        }
        let mut excluded_ids = BTreeSet::new();
        for exclusion in &self.exclusions {
            if included_ids.contains(&exclusion.item_id)
                || !excluded_ids.insert(exclusion.item_id.clone())
            {
                return Err(DomainError::InvalidValue("context.exclusions"));
            }
        }
        if self
            .approved_at
            .is_some_and(|approved| approved < self.created_at)
        {
            return Err(DomainError::InvalidValue("context.approved_at"));
        }
        if self.dispatched_at.is_some_and(|dispatched| {
            dispatched < self.created_at
                || self
                    .approved_at
                    .is_some_and(|approved| dispatched < approved)
        }) {
            return Err(DomainError::InvalidValue("context.dispatched_at"));
        }
        if self.dispatched_at.is_some() && self.requires_approval && self.approved_at.is_none() {
            return Err(DomainError::InvalidValue("context.approved_at"));
        }
        Ok(())
    }
}

/// Deterministic minimum-context builder. Search ranking never bypasses policy filtering.
#[derive(Debug, Clone, Copy)]
pub struct ContextBuilder {
    maximum_items: usize,
}

impl ContextBuilder {
    /// Construct a builder with a hard result cap no greater than the public schema.
    pub fn new(maximum_items: usize) -> Result<Self, DomainError> {
        if maximum_items == 0 || maximum_items > 64 {
            return Err(DomainError::InvalidValue("context.maximum_items"));
        }
        Ok(Self { maximum_items })
    }

    /// Build an auditable preview packet.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        self,
        actor: Actor,
        purpose: impl Into<String>,
        method: impl Into<String>,
        provider_profile_id: EntityId,
        model: impl Into<String>,
        query: impl Into<String>,
        mut candidates: Vec<MemoryCandidate>,
        created_at: DateTime<Utc>,
    ) -> Result<ContextPacket, DomainError> {
        actor.require(Capability::BuildContext)?;
        if candidates.len() > 128 {
            return Err(DomainError::InvalidValue("context.candidate_limit"));
        }
        candidates.sort_by_key(|candidate| {
            (
                Reverse(candidate.item_type == ContextItemType::Correction),
                Reverse(candidate.relevance_score),
                Reverse(candidate.recorded_at),
                candidate.item_id.clone(),
            )
        });

        let mut items = Vec::new();
        let mut exclusions = Vec::new();
        for candidate in candidates {
            if let Some(reason) = exclusion_reason(&candidate) {
                exclusions.push(ContextExclusion {
                    item_id: candidate.item_id,
                    reason,
                });
                continue;
            }
            if items.len() == self.maximum_items {
                exclusions.push(ContextExclusion {
                    item_id: candidate.item_id,
                    reason: ExclusionReason::ResultLimit,
                });
                continue;
            }
            validate_text(&candidate.excerpt, 8_000, "context.excerpt")?;
            validate_text(&candidate.selection_reason, 500, "context.selection_reason")?;
            items.push(ContextItem {
                item_type: candidate.item_type,
                item_id: candidate.item_id,
                excerpt: candidate.excerpt,
                sensitivity: candidate.sensitivity,
                selection_reason: candidate.selection_reason,
            });
        }

        let requires_approval = items
            .iter()
            .any(|item| item.sensitivity.requires_explicit_approval());
        let packet = ContextPacket {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Context),
            purpose: purpose.into(),
            method: method.into(),
            provider_profile_id,
            model: model.into(),
            query: query.into(),
            items,
            exclusions,
            requires_approval,
            approved_at: None,
            created_at,
            dispatched_at: None,
        };
        packet.validate()?;
        Ok(packet)
    }
}

fn exclusion_reason(candidate: &MemoryCandidate) -> Option<ExclusionReason> {
    if candidate.sensitivity == Sensitivity::LocalOnly {
        return Some(ExclusionReason::LocalOnly);
    }
    if candidate.crisis_related {
        return Some(ExclusionReason::CrisisRelated);
    }
    match candidate.state {
        MemoryState::PendingReview => return Some(ExclusionReason::PendingReview),
        MemoryState::Rejected => return Some(ExclusionReason::Rejected),
        MemoryState::Deprecated => return Some(ExclusionReason::Deprecated),
        MemoryState::Deleted => return Some(ExclusionReason::Deleted),
        MemoryState::Expired => return Some(ExclusionReason::Expired),
        MemoryState::Confirmed | MemoryState::Active => {}
    }
    if !candidate.relevant {
        return Some(ExclusionReason::NotRelevant);
    }
    None
}

const fn entity_kind(item_type: ContextItemType) -> EntityKind {
    match item_type {
        ContextItemType::Message => EntityKind::Message,
        ContextItemType::SessionSummary => EntityKind::SessionSummary,
        ContextItemType::Event => EntityKind::Event,
        ContextItemType::Claim => EntityKind::Claim,
        ContextItemType::Correction => EntityKind::Correction,
        ContextItemType::OpenQuestion => EntityKind::OpenQuestion,
        ContextItemType::Experiment => EntityKind::Experiment,
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    fn candidate(
        item_type: ContextItemType,
        sensitivity: Sensitivity,
        state: MemoryState,
        relevance_score: u16,
    ) -> MemoryCandidate {
        MemoryCandidate {
            item_type,
            item_id: EntityId::new(match item_type {
                ContextItemType::Message => EntityKind::Message,
                ContextItemType::SessionSummary => EntityKind::SessionSummary,
                ContextItemType::Event => EntityKind::Event,
                ContextItemType::Claim => EntityKind::Claim,
                ContextItemType::Correction => EntityKind::Correction,
                ContextItemType::OpenQuestion => EntityKind::OpenQuestion,
                ContextItemType::Experiment => EntityKind::Experiment,
            }),
            excerpt: "Synthetic excerpt.".to_owned(),
            sensitivity,
            state,
            crisis_related: false,
            relevant: true,
            selection_reason: "Synthetic relevance.".to_owned(),
            relevance_score,
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn local_only_pending_rejected_and_crisis_content_are_excluded() -> Result<(), Box<dyn Error>> {
        let builder = ContextBuilder::new(10)?;
        let mut crisis = candidate(
            ContextItemType::Event,
            Sensitivity::Private,
            MemoryState::Confirmed,
            100,
        );
        crisis.crisis_related = true;
        let packet = builder.build(
            Actor::model_host(),
            "Synthetic work reflection",
            "event-debrief@1.0.0",
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "A synthetic query",
            vec![
                candidate(
                    ContextItemType::Claim,
                    Sensitivity::LocalOnly,
                    MemoryState::Confirmed,
                    100,
                ),
                candidate(
                    ContextItemType::Claim,
                    Sensitivity::Private,
                    MemoryState::PendingReview,
                    100,
                ),
                candidate(
                    ContextItemType::Claim,
                    Sensitivity::Private,
                    MemoryState::Rejected,
                    100,
                ),
                crisis,
            ],
            Utc::now(),
        )?;
        assert!(packet.items.is_empty());
        assert_eq!(packet.exclusions.len(), 4);
        Ok(())
    }

    #[test]
    fn correction_priority_precedes_raw_relevance_rank() -> Result<(), Box<dyn Error>> {
        let builder = ContextBuilder::new(1)?;
        let correction = candidate(
            ContextItemType::Correction,
            Sensitivity::Private,
            MemoryState::Active,
            1,
        );
        let correction_id = correction.item_id.clone();
        let claim = candidate(
            ContextItemType::Claim,
            Sensitivity::Private,
            MemoryState::Confirmed,
            1_000,
        );
        let packet = builder.build(
            Actor::model_host(),
            "Synthetic purpose",
            "free-reflection@1.0.0",
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "Synthetic query",
            vec![claim, correction],
            Utc::now(),
        )?;
        assert_eq!(packet.items[0].item_id, correction_id);
        Ok(())
    }

    #[test]
    fn highly_sensitive_preview_requires_human_approval_before_egress() -> Result<(), Box<dyn Error>>
    {
        let builder = ContextBuilder::new(4)?;
        let mut packet = builder.build(
            Actor::model_host(),
            "Synthetic purpose",
            "free-reflection@1.0.0",
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "Synthetic query",
            vec![candidate(
                ContextItemType::Event,
                Sensitivity::HighlySensitive,
                MemoryState::Confirmed,
                100,
            )],
            Utc::now(),
        )?;
        assert!(packet.requires_approval);
        assert!(packet.authorize_egress().is_err());
        packet.approve(Actor::user(), Utc::now())?;
        packet.authorize_egress()?;
        Ok(())
    }

    #[test]
    fn model_host_cannot_mark_a_context_packet_dispatched() -> Result<(), Box<dyn Error>> {
        let builder = ContextBuilder::new(4)?;
        let mut packet = builder.build(
            Actor::model_host(),
            "Synthetic purpose",
            "free-reflection@1.0.0",
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "Synthetic query",
            vec![candidate(
                ContextItemType::Claim,
                Sensitivity::Private,
                MemoryState::Confirmed,
                100,
            )],
            Utc::now(),
        )?;
        assert_eq!(
            packet.mark_dispatched(Actor::model_host(), Utc::now()),
            Err(DomainError::Forbidden(Capability::BuildContext))
        );
        packet.mark_dispatched(Actor::system(), Utc::now())?;
        assert!(packet.dispatched_at.is_some());
        Ok(())
    }
}
