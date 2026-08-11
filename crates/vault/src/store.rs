use chrono::{DateTime, SecondsFormat, Utc};
use quisum_core::{
    Actor, ActorKind, AuditAction, AuditDetails, AuditEvent, BehavioralExperiment, Capability,
    Claim, ClaimKind, ClaimRevision, ClaimStatus, ContextItemType, ContextPacket, EntityId,
    EntityKind, Event, EventStatus, ExperimentStatus, OpenQuestion, ReviewOutcome, ReviewRequest,
    SafetyEngine, Source, SourceActor, UserCorrection, UserReview, Validate,
};
use rusqlite::{ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::{SqlCipherVault, VaultError};

/// A counterevidence link awaiting human review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceProposal {
    /// Claim to be limited or contradicted.
    pub claim_id: EntityId,
    /// Candidate evidence source.
    pub evidence_id: EntityId,
    /// Proposing actor.
    pub actor: ActorKind,
    /// Auditable context that exposed both records to a model host.
    pub context_packet_id: Option<EntityId>,
    /// Proposal time.
    pub created_at: DateTime<Utc>,
}

/// Result of deleting a source event and repairing dependent claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionImpact {
    /// Deleted event.
    pub event_id: EntityId,
    /// Confirmed or otherwise retained claims moved to a conservative projection.
    pub affected_claims: Vec<EntityId>,
    /// Unreviewed claims whose only support was the deleted event and were purged.
    pub purged_pending_claims: Vec<EntityId>,
}

/// Human review persisted in one transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCommit {
    /// Domain review and revision.
    pub outcome: ReviewOutcome,
    /// Open question created by conversion, if any.
    pub open_question: Option<OpenQuestion>,
}

impl SqlCipherVault {
    /// Persist an immutable context selection preview before approval or egress.
    pub fn insert_context_packet(
        &self,
        actor: Actor,
        packet: &ContextPacket,
    ) -> Result<(), VaultError> {
        actor.require(Capability::MaintainVault)?;
        if actor.kind() != ActorKind::System {
            return Err(quisum_core::DomainError::Forbidden(Capability::MaintainVault).into());
        }
        packet.validate()?;
        if packet.approved_at.is_some() || packet.dispatched_at.is_some() {
            return Err(quisum_core::DomainError::InvalidValue("context.initial_state").into());
        }

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_context_items_eligible(&transaction, packet)?;
        transaction
            .execute(
                "INSERT INTO context_packets(
                   id, provider_profile_id, purpose, requires_approval,
                   approved_at, created_at, dispatched_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    packet.id.to_string(),
                    packet.provider_profile_id.to_string(),
                    packet.purpose,
                    i64::from(packet.requires_approval),
                    Option::<String>::None,
                    format_time(packet.created_at),
                    Option::<String>::None,
                    serde_json::to_string(packet)?
                ],
            )
            .map_err(map_constraint)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ContextBuilt,
            Some(packet.id.clone()),
            AuditDetails::AffectedRecords {
                count: u64::try_from(packet.items.len())
                    .map_err(|_| quisum_core::DomainError::InvalidValue("context.item_count"))?,
            },
            packet.created_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }

    /// Record one-time user approval for a highly sensitive context packet.
    pub fn approve_context_packet(
        &self,
        actor: Actor,
        packet_id: &EntityId,
        approved_at: DateTime<Utc>,
    ) -> Result<ContextPacket, VaultError> {
        actor.require(Capability::ChangePrivacyPolicy)?;
        if actor.kind() != ActorKind::User {
            return Err(
                quisum_core::DomainError::Forbidden(Capability::ChangePrivacyPolicy).into(),
            );
        }
        packet_id.require_kind(EntityKind::Context)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut packet = load_context_packet(&transaction, packet_id)?;
        packet.approve(actor, approved_at)?;
        update_context_packet(&transaction, &packet)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ContextApproved,
            Some(packet.id.clone()),
            AuditDetails::None,
            approved_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(packet)
    }

    /// Perform final egress validation and record dispatch atomically before provider I/O.
    pub fn mark_context_packet_dispatched(
        &self,
        actor: Actor,
        packet_id: &EntityId,
        dispatched_at: DateTime<Utc>,
    ) -> Result<ContextPacket, VaultError> {
        actor.require(Capability::BuildContext)?;
        if !matches!(actor.kind(), ActorKind::User | ActorKind::System) {
            return Err(quisum_core::DomainError::Forbidden(Capability::BuildContext).into());
        }
        packet_id.require_kind(EntityKind::Context)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut packet = load_context_packet(&transaction, packet_id)?;
        packet.mark_dispatched(actor, dispatched_at)?;
        update_context_packet(&transaction, &packet)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ContextDispatched,
            Some(packet.id.clone()),
            AuditDetails::AffectedRecords {
                count: u64::try_from(packet.items.len())
                    .map_err(|_| quisum_core::DomainError::InvalidValue("context.item_count"))?,
            },
            dispatched_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(packet)
    }

    /// Persist a concrete event while enforcing actor/source and crisis rules.
    pub fn insert_event(&self, actor: Actor, event: &Event) -> Result<(), VaultError> {
        actor.require(Capability::ProposeEvent)?;
        event.validate()?;
        require_source_actor(actor.kind(), event.source.actor)?;
        if event.crisis_related {
            return Err(quisum_core::DomainError::SafetyBlocked.into());
        }
        match actor.kind() {
            ActorKind::ModelHost | ActorKind::Import if event.status != EventStatus::Proposed => {
                return Err(quisum_core::DomainError::InvalidValue("event.status").into());
            }
            ActorKind::User if event.status != EventStatus::Confirmed => {
                return Err(quisum_core::DomainError::InvalidValue("event.status").into());
            }
            ActorKind::User | ActorKind::System | ActorKind::ModelHost | ActorKind::Import => {}
        }

        let payload = serde_json::to_string(event)?;
        require_safe_model_output(&event.source, &payload)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let _model_context = require_dispatched_model_context(&transaction, &event.source)?;
        let status = enum_token(&event.status)?;
        let sensitivity = enum_token(&event.sensitivity)?;
        transaction
            .execute(
                "INSERT INTO events(
                   id, status, title, occurred_at, recorded_at,
                   sensitivity, crisis_related, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    event.id.to_string(),
                    status,
                    event.title,
                    event.occurred_at.map(format_time),
                    format_time(event.recorded_at),
                    sensitivity,
                    i64::from(event.crisis_related),
                    payload
                ],
            )
            .map_err(map_constraint)?;
        if event.status == EventStatus::Confirmed {
            insert_event_search_index(&transaction, event)?;
        }
        let audit = AuditEvent::new(
            actor.kind(),
            if event.status == EventStatus::Proposed {
                AuditAction::CandidateProposed
            } else {
                AuditAction::EventRecorded
            },
            Some(event.id.clone()),
            AuditDetails::None,
            event.recorded_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }

    /// Accept or reject a proposed event. The model cannot review its own proposal.
    pub fn review_event(
        &self,
        actor: Actor,
        event_id: &EntityId,
        accepted: bool,
        reviewed_at: DateTime<Utc>,
    ) -> Result<Event, VaultError> {
        actor.require(Capability::ReviewEvent)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewEvent).into());
        }
        event_id.require_kind(EntityKind::Event)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload: String = transaction
            .query_row(
                "SELECT payload_json FROM events WHERE id = ?1",
                [event_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(VaultError::NotFoundRecord)?;
        let mut event: Event = serde_json::from_str(&payload)?;
        if event.status != EventStatus::Proposed {
            return Err(quisum_core::DomainError::InvalidValue("event.status").into());
        }
        if reviewed_at < event.recorded_at {
            return Err(quisum_core::DomainError::InvalidValue("event.reviewed_at").into());
        }
        let previous = event.status;
        event.status = if accepted {
            EventStatus::Confirmed
        } else {
            EventStatus::Rejected
        };
        event.validate()?;
        let status = enum_token(&event.status)?;
        let payload = serde_json::to_string(&event)?;
        transaction.execute(
            "UPDATE events SET status = ?2, payload_json = ?3 WHERE id = ?1",
            params![event.id.to_string(), status, payload],
        )?;
        transaction.execute(
            "DELETE FROM events_fts WHERE id = ?1",
            [event.id.to_string()],
        )?;
        if event.status == EventStatus::Confirmed {
            insert_event_search_index(&transaction, &event)?;
        }
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::EventReviewed,
            Some(event.id.clone()),
            AuditDetails::Lifecycle {
                from: enum_token(&previous)?,
                to: enum_token(&event.status)?,
            },
            reviewed_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(event)
    }

    /// Persist a candidate claim. Only reviewed source records may be evidence.
    pub fn insert_claim(&self, actor: Actor, claim: &Claim) -> Result<(), VaultError> {
        actor.require(Capability::ProposeClaim)?;
        claim.validate()?;
        require_source_actor(actor.kind(), claim.source.actor)?;
        if claim.crisis_related {
            return Err(quisum_core::DomainError::SafetyBlocked.into());
        }
        match actor.kind() {
            ActorKind::ModelHost | ActorKind::Import
                if claim.status != ClaimStatus::PendingReview =>
            {
                return Err(quisum_core::DomainError::InvalidValue("claim.status").into());
            }
            ActorKind::User
                if !matches!(
                    claim.status,
                    ClaimStatus::PendingReview | ClaimStatus::Confirmed | ClaimStatus::Revised
                ) =>
            {
                return Err(quisum_core::DomainError::InvalidValue("claim.status").into());
            }
            ActorKind::User | ActorKind::System | ActorKind::ModelHost | ActorKind::Import => {}
        }

        let payload = serde_json::to_string(claim)?;
        require_safe_model_output(&claim.source, &payload)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let model_context = require_dispatched_model_context(&transaction, &claim.source)?;
        for evidence in claim
            .supporting_evidence
            .iter()
            .chain(claim.counterevidence.iter())
        {
            require_eligible_evidence(&transaction, evidence)?;
            if model_context
                .as_ref()
                .is_some_and(|packet| !packet.items.iter().any(|item| item.item_id == *evidence))
            {
                return Err(
                    quisum_core::DomainError::InvalidValue("claim.evidence_provenance").into(),
                );
            }
        }
        insert_claim_projection(&transaction, claim)?;
        for evidence in &claim.supporting_evidence {
            insert_evidence_link(
                &transaction,
                NewEvidenceLink {
                    claim_id: &claim.id,
                    evidence_id: evidence,
                    relation: "supporting",
                    review_status: "confirmed",
                    actor: actor.kind(),
                    context_packet_id: claim.source.context_packet_id.as_ref(),
                    created_at: claim.created_at,
                },
            )?;
        }
        for evidence in &claim.counterevidence {
            insert_evidence_link(
                &transaction,
                NewEvidenceLink {
                    claim_id: &claim.id,
                    evidence_id: evidence,
                    relation: "counter",
                    review_status: "confirmed",
                    actor: actor.kind(),
                    context_packet_id: claim.source.context_packet_id.as_ref(),
                    created_at: claim.created_at,
                },
            )?;
        }
        let audit = AuditEvent::new(
            actor.kind(),
            if actor.kind() == ActorKind::User
                && matches!(claim.status, ClaimStatus::Confirmed | ClaimStatus::Revised)
            {
                AuditAction::ClaimRecorded
            } else {
                AuditAction::CandidateProposed
            },
            Some(claim.id.clone()),
            AuditDetails::None,
            claim.created_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }

    /// Read one claim through the actor's bounded-record capability.
    pub fn get_claim(
        &self,
        actor: Actor,
        claim_id: &EntityId,
    ) -> Result<Option<Claim>, VaultError> {
        actor.require(Capability::ReadBoundedRecord)?;
        claim_id.require_kind(EntityKind::Claim)?;
        let connection = self.connection.lock();
        let payload = connection
            .query_row(
                "SELECT payload_json FROM claims WHERE id = ?1",
                [claim_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let claim: Claim = serde_json::from_str(&payload)?;
        if actor.kind() == ActorKind::ModelHost
            && (matches!(
                claim.sensitivity,
                quisum_core::Sensitivity::HighlySensitive | quisum_core::Sensitivity::LocalOnly
            ) || claim.crisis_related
                || !matches!(
                    claim.status,
                    ClaimStatus::Confirmed | ClaimStatus::Revised | ClaimStatus::Contradicted
                ))
        {
            return Ok(None);
        }
        claim.validate()?;
        Ok(Some(claim))
    }

    /// List the user-visible review queue in deterministic recency order.
    pub fn list_review_claims(&self, actor: Actor, limit: usize) -> Result<Vec<Claim>, VaultError> {
        actor.require(Capability::ReviewClaim)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewClaim).into());
        }
        if limit == 0 || limit > 100 {
            return Err(quisum_core::DomainError::InvalidValue("claim.list_limit").into());
        }
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT payload_json FROM claims
             WHERE status IN ('pending_review', 'uncertain', 'contradicted')
             ORDER BY updated_at DESC, id
             LIMIT ?1",
        )?;
        let rows = statement.query_map(
            [i64::try_from(limit)
                .map_err(|_| quisum_core::DomainError::InvalidValue("claim.list_limit"))?],
            |row| row.get::<_, String>(0),
        )?;
        let mut claims = Vec::new();
        for row in rows {
            let claim: Claim = serde_json::from_str(&row?)?;
            claim.validate()?;
            claims.push(claim);
        }
        Ok(claims)
    }

    /// Apply and persist a human claim review atomically with its revision.
    pub fn review_claim(
        &self,
        actor: Actor,
        claim_id: &EntityId,
        request: ReviewRequest,
        reviewed_at: DateTime<Utc>,
    ) -> Result<ReviewCommit, VaultError> {
        actor.require(Capability::ReviewClaim)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewClaim).into());
        }
        claim_id.require_kind(EntityKind::Claim)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut claim = load_claim(&transaction, claim_id)?;
        let previous_status = claim.status;
        let outcome = claim.apply_review(actor, request, reviewed_at)?;
        update_claim_projection(&transaction, &claim)?;
        insert_review(&transaction, &outcome.review)?;
        insert_revision(&transaction, &outcome.revision)?;

        let open_question = outcome.converted_question.as_ref().map(|question| {
            let mut open_question = OpenQuestion::from_reviewed_claim(
                question,
                claim.id.clone(),
                claim.sensitivity,
                reviewed_at,
            );
            open_question.contexts = claim.contexts.clone();
            open_question
        });
        if let Some(open_question) = &open_question {
            open_question.validate()?;
            insert_open_question(&transaction, open_question)?;
        }

        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ClaimReviewed,
            Some(claim.id.clone()),
            AuditDetails::Lifecycle {
                from: enum_token(&previous_status)?,
                to: enum_token(&claim.status)?,
            },
            reviewed_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(ReviewCommit {
            outcome,
            open_question,
        })
    }

    /// Record a model/user counterevidence proposal without mutating the confirmed claim.
    pub fn propose_counterevidence(
        &self,
        actor: Actor,
        claim_id: &EntityId,
        evidence_id: &EntityId,
        context_packet_id: Option<&EntityId>,
        created_at: DateTime<Utc>,
    ) -> Result<EvidenceProposal, VaultError> {
        actor.require(Capability::AddCounterevidence)?;
        claim_id.require_kind(EntityKind::Claim)?;
        require_eligible_evidence_id(evidence_id)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let context_packet_id = match actor.kind() {
            ActorKind::ModelHost => {
                let packet_id = context_packet_id.ok_or(quisum_core::DomainError::InvalidValue(
                    "evidence.context_packet_id",
                ))?;
                require_dispatched_context_contains(
                    &transaction,
                    packet_id,
                    &[claim_id, evidence_id],
                )?;
                Some(packet_id)
            }
            ActorKind::User | ActorKind::System => {
                if context_packet_id.is_some() {
                    return Err(quisum_core::DomainError::InvalidValue(
                        "evidence.context_packet_id",
                    )
                    .into());
                }
                None
            }
            ActorKind::Import => {
                return Err(
                    quisum_core::DomainError::Forbidden(Capability::AddCounterevidence).into(),
                );
            }
        };
        require_eligible_evidence(&transaction, evidence_id)?;
        let claim = load_claim(&transaction, claim_id)?;
        if claim.supporting_evidence.contains(evidence_id)
            || claim.counterevidence.contains(evidence_id)
        {
            return Err(quisum_core::DomainError::InvalidValue("claim.evidence_overlap").into());
        }
        insert_evidence_link(
            &transaction,
            NewEvidenceLink {
                claim_id,
                evidence_id,
                relation: "counter",
                review_status: "proposed",
                actor: actor.kind(),
                context_packet_id,
                created_at,
            },
        )?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::CandidateProposed,
            Some(claim_id.clone()),
            AuditDetails::None,
            created_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(EvidenceProposal {
            claim_id: claim_id.clone(),
            evidence_id: evidence_id.clone(),
            actor: actor.kind(),
            context_packet_id: context_packet_id.cloned(),
            created_at,
        })
    }

    /// Accept proposed counterevidence and conservatively flag a confirmed claim.
    pub fn accept_counterevidence(
        &self,
        actor: Actor,
        claim_id: &EntityId,
        evidence_id: &EntityId,
        reviewed_at: DateTime<Utc>,
    ) -> Result<Claim, VaultError> {
        actor.require(Capability::ReviewClaim)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewClaim).into());
        }
        claim_id.require_kind(EntityKind::Claim)?;
        require_eligible_evidence_id(evidence_id)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let proposal_exists: bool = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM evidence_links
               WHERE claim_id = ?1 AND evidence_id = ?2
                 AND relation = 'counter' AND review_status = 'proposed'
             )",
            params![claim_id.to_string(), evidence_id.to_string()],
            |row| row.get(0),
        )?;
        if !proposal_exists {
            return Err(VaultError::NotFoundRecord);
        }
        require_eligible_evidence(&transaction, evidence_id)?;
        let mut claim = load_claim(&transaction, claim_id)?;
        let revision = claim.add_counterevidence(actor, evidence_id.clone(), reviewed_at)?;
        update_claim_projection(&transaction, &claim)?;
        transaction.execute(
            "UPDATE evidence_links
             SET review_status = 'confirmed', source_actor = 'user'
             WHERE claim_id = ?1 AND evidence_id = ?2 AND relation = 'counter'",
            params![claim_id.to_string(), evidence_id.to_string()],
        )?;
        insert_revision(&transaction, &revision)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::EvidenceAdded,
            Some(claim.id.clone()),
            AuditDetails::Lifecycle {
                from: enum_token(&revision.before.status)?,
                to: enum_token(&revision.after.status)?,
            },
            reviewed_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(claim)
    }

    /// Delete an event and repair or purge every dependent claim in one transaction.
    pub fn delete_event(
        &self,
        actor: Actor,
        event_id: &EntityId,
        deleted_at: DateTime<Utc>,
    ) -> Result<DeletionImpact, VaultError> {
        actor.require(Capability::DeleteData)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::DeleteData).into());
        }
        event_id.require_kind(EntityKind::Event)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let event_payload = transaction
            .query_row(
                "SELECT payload_json FROM events WHERE id = ?1",
                [event_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(VaultError::NotFoundRecord)?;
        let event: Event = serde_json::from_str(&event_payload)?;
        event.validate()?;
        if deleted_at < event.recorded_at {
            return Err(quisum_core::DomainError::InvalidValue("event.deleted_at").into());
        }

        let dependent_payloads = {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT c.payload_json
                 FROM claims c
                 JOIN evidence_links e ON e.claim_id = c.id
                 WHERE e.evidence_id = ?1",
            )?;
            let rows =
                statement.query_map([event_id.to_string()], |row| row.get::<_, String>(0))?;
            let mut payloads = Vec::new();
            for row in rows {
                payloads.push(row?);
            }
            payloads
        };

        let mut affected_claims = Vec::new();
        let mut purged_pending_claims = Vec::new();
        for payload in dependent_payloads {
            let mut claim: Claim = serde_json::from_str(&payload)?;
            let purge_unreviewed = matches!(
                claim.status,
                ClaimStatus::Proposed | ClaimStatus::PendingReview
            ) && claim.supporting_evidence.len() == 1
                && claim.supporting_evidence.contains(event_id);
            if purge_unreviewed {
                transaction.execute("DELETE FROM claims WHERE id = ?1", [claim.id.to_string()])?;
                transaction.execute(
                    "DELETE FROM claims_fts WHERE id = ?1",
                    [claim.id.to_string()],
                )?;
                purged_pending_claims.push(claim.id);
                continue;
            }
            if let Some(revision) = claim.remove_evidence(actor, event_id, deleted_at)? {
                update_claim_projection(&transaction, &claim)?;
                insert_revision(&transaction, &revision)?;
                affected_claims.push(claim.id);
            }
        }

        transaction.execute(
            "DELETE FROM evidence_links WHERE evidence_id = ?1",
            [event_id.to_string()],
        )?;
        transaction.execute("DELETE FROM events WHERE id = ?1", [event_id.to_string()])?;
        transaction.execute(
            "DELETE FROM events_fts WHERE id = ?1",
            [event_id.to_string()],
        )?;
        let affected_count = affected_claims.len() + purged_pending_claims.len();
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::EntityDeleted,
            Some(event_id.clone()),
            AuditDetails::AffectedRecords {
                count: u64::try_from(affected_count).map_err(|_| {
                    quisum_core::DomainError::InvalidValue("deletion.affected_count")
                })?,
            },
            deleted_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        affected_claims.sort();
        purged_pending_claims.sort();
        Ok(DeletionImpact {
            event_id: event_id.clone(),
            affected_claims,
            purged_pending_claims,
        })
    }

    /// Record a direct user correction. Model hosts have no equivalent operation.
    pub fn insert_correction(
        &self,
        actor: Actor,
        correction: &UserCorrection,
    ) -> Result<(), VaultError> {
        actor.require(Capability::RecordCorrection)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::RecordCorrection).into());
        }
        correction.validate()?;
        if !correction.active {
            return Err(quisum_core::DomainError::InvalidValue("correction.active").into());
        }
        require_source_actor(actor.kind(), correction.source.actor)?;
        let payload = serde_json::to_string(correction)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction
            .execute(
                "INSERT INTO user_corrections(
                   id, active, sensitivity, created_at, updated_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    correction.id.to_string(),
                    i64::from(correction.active),
                    enum_token(&correction.sensitivity)?,
                    format_time(correction.created_at),
                    format_time(correction.updated_at),
                    payload
                ],
            )
            .map_err(map_constraint)?;
        transaction.execute(
            "INSERT INTO corrections_fts(id, statement, topics) VALUES (?1, ?2, ?3)",
            params![
                correction.id.to_string(),
                correction.statement,
                correction
                    .topics
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            ],
        )?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::CorrectionRecorded,
            Some(correction.id.clone()),
            AuditDetails::None,
            correction.created_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }

    /// Persist a behavioral-experiment proposal or user-reviewed state.
    pub fn insert_experiment(
        &self,
        actor: Actor,
        experiment: &BehavioralExperiment,
    ) -> Result<(), VaultError> {
        actor.require(Capability::ProposeExperiment)?;
        experiment.validate()?;
        require_source_actor(actor.kind(), experiment.source.actor)?;
        if actor.kind() != ActorKind::System && experiment.status != ExperimentStatus::Proposed {
            return Err(quisum_core::DomainError::InvalidValue("experiment.status").into());
        }
        let payload = serde_json::to_string(experiment)?;
        require_safe_model_output(&experiment.source, &payload)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let model_context = require_dispatched_model_context(&transaction, &experiment.source)?;
        for claim_id in &experiment.related_claims {
            let claim = load_claim(&transaction, claim_id)?;
            if claim.crisis_related
                || !matches!(
                    claim.status,
                    ClaimStatus::Confirmed | ClaimStatus::Revised | ClaimStatus::Contradicted
                )
            {
                return Err(
                    quisum_core::DomainError::InvalidValue("experiment.related_claims").into(),
                );
            }
            if model_context
                .as_ref()
                .is_some_and(|packet| !packet.items.iter().any(|item| item.item_id == *claim_id))
            {
                return Err(
                    quisum_core::DomainError::InvalidValue("experiment.claim_provenance").into(),
                );
            }
        }
        transaction
            .execute(
                "INSERT INTO experiments(
                   id, status, risk_level, planned_at, completed_at,
                   sensitivity, crisis_related, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    experiment.id.to_string(),
                    enum_token(&experiment.status)?,
                    enum_token(&experiment.risk_level)?,
                    format_time(experiment.planned_at),
                    experiment.completed_at.map(format_time),
                    enum_token(&experiment.sensitivity)?,
                    i64::from(experiment.crisis_related),
                    payload
                ],
            )
            .map_err(map_constraint)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::CandidateProposed,
            Some(experiment.id.clone()),
            AuditDetails::None,
            experiment.planned_at,
        );
        insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }

    /// Activate a proposed experiment through an explicit human review.
    pub fn activate_experiment(
        &self,
        actor: Actor,
        experiment_id: &EntityId,
        reviewed_at: DateTime<Utc>,
    ) -> Result<BehavioralExperiment, VaultError> {
        actor.require(Capability::ReviewExperiment)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewExperiment).into());
        }
        experiment_id.require_kind(EntityKind::Experiment)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut experiment = load_experiment(&transaction, experiment_id)?;
        if reviewed_at < experiment.planned_at {
            return Err(quisum_core::DomainError::InvalidValue("experiment.reviewed_at").into());
        }
        let previous = experiment.status;
        experiment.activate(actor)?;
        update_experiment(&transaction, &experiment)?;
        insert_experiment_audit(&transaction, &experiment, previous, reviewed_at)?;
        transaction.commit()?;
        Ok(experiment)
    }

    /// Complete an active experiment while preserving its advance prediction.
    pub fn complete_experiment(
        &self,
        actor: Actor,
        experiment_id: &EntityId,
        actual_result: impl Into<String>,
        learning: impl Into<String>,
        completed_at: DateTime<Utc>,
    ) -> Result<BehavioralExperiment, VaultError> {
        actor.require(Capability::ReviewExperiment)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewExperiment).into());
        }
        experiment_id.require_kind(EntityKind::Experiment)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut experiment = load_experiment(&transaction, experiment_id)?;
        let previous = experiment.status;
        experiment.complete(actor, actual_result, learning, completed_at)?;
        update_experiment(&transaction, &experiment)?;
        insert_experiment_audit(&transaction, &experiment, previous, completed_at)?;
        transaction.commit()?;
        Ok(experiment)
    }

    /// Cancel a proposed or active experiment without penalty.
    pub fn cancel_experiment(
        &self,
        actor: Actor,
        experiment_id: &EntityId,
        cancelled_at: DateTime<Utc>,
    ) -> Result<BehavioralExperiment, VaultError> {
        actor.require(Capability::ReviewExperiment)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::ReviewExperiment).into());
        }
        experiment_id.require_kind(EntityKind::Experiment)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut experiment = load_experiment(&transaction, experiment_id)?;
        if cancelled_at < experiment.planned_at {
            return Err(quisum_core::DomainError::InvalidValue("experiment.cancelled_at").into());
        }
        let previous = experiment.status;
        experiment.cancel(actor)?;
        update_experiment(&transaction, &experiment)?;
        insert_experiment_audit(&transaction, &experiment, previous, cancelled_at)?;
        transaction.commit()?;
        Ok(experiment)
    }

    /// Read one experiment through the actor's bounded-record capability.
    pub fn get_experiment(
        &self,
        actor: Actor,
        experiment_id: &EntityId,
    ) -> Result<Option<BehavioralExperiment>, VaultError> {
        actor.require(Capability::ReadBoundedRecord)?;
        experiment_id.require_kind(EntityKind::Experiment)?;
        let connection = self.connection.lock();
        let payload = connection
            .query_row(
                "SELECT payload_json FROM experiments WHERE id = ?1",
                [experiment_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let experiment: BehavioralExperiment = serde_json::from_str(&payload)?;
        experiment.validate()?;
        if actor.kind() == ActorKind::ModelHost
            && (matches!(
                experiment.sensitivity,
                quisum_core::Sensitivity::HighlySensitive | quisum_core::Sensitivity::LocalOnly
            ) || experiment.crisis_related
                || !matches!(
                    experiment.status,
                    ExperimentStatus::Active | ExperimentStatus::Completed
                ))
        {
            return Ok(None);
        }
        Ok(Some(experiment))
    }

    /// Count append-only revisions for integrity tests and reflog summaries.
    pub fn claim_revision_count(&self, claim_id: &EntityId) -> Result<u64, VaultError> {
        claim_id.require_kind(EntityKind::Claim)?;
        let connection = self.connection.lock();
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM claim_revisions WHERE claim_id = ?1",
            [claim_id.to_string()],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| VaultError::IntegrityCheckFailed)
    }

    /// Count structural audit records without exposing their contents.
    pub fn audit_count(&self) -> Result<u64, VaultError> {
        let connection = self.connection.lock();
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
        u64::try_from(count).map_err(|_| VaultError::IntegrityCheckFailed)
    }
}

fn require_source_actor(actor: ActorKind, source: SourceActor) -> Result<(), VaultError> {
    let matches = matches!(
        (actor, source),
        (ActorKind::User, SourceActor::User)
            | (ActorKind::ModelHost, SourceActor::Model)
            | (ActorKind::System, SourceActor::System)
            | (ActorKind::Import, SourceActor::Import)
    );
    if matches {
        Ok(())
    } else {
        Err(quisum_core::DomainError::InvalidValue("source.actor").into())
    }
}

fn require_safe_model_output(source: &Source, serialized: &str) -> Result<(), VaultError> {
    if source.actor == SourceActor::Model {
        let decision = SafetyEngine.review_generated_text(serialized);
        SafetyEngine.require_persistence_allowed(&decision)?;
    }
    Ok(())
}

fn load_context_packet(
    transaction: &Transaction<'_>,
    packet_id: &EntityId,
) -> Result<ContextPacket, VaultError> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM context_packets WHERE id = ?1",
            [packet_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(VaultError::NotFoundRecord)?;
    let packet: ContextPacket = serde_json::from_str(&payload)?;
    packet.validate()?;
    Ok(packet)
}

fn require_context_items_eligible(
    transaction: &Transaction<'_>,
    packet: &ContextPacket,
) -> Result<(), VaultError> {
    for item in &packet.items {
        let sensitivity = enum_token(&item.sensitivity)?;
        let query = match item.item_type {
            ContextItemType::Message => {
                "SELECT payload_json FROM messages
                 WHERE id = ?1 AND sensitivity = ?2 AND crisis_related = 0"
            }
            ContextItemType::SessionSummary => {
                "SELECT payload_json FROM session_summaries
                 WHERE id = ?1 AND review_status = 'confirmed'
                   AND sensitivity = ?2 AND crisis_related = 0"
            }
            ContextItemType::Event => {
                "SELECT payload_json FROM events
                 WHERE id = ?1 AND status = 'confirmed'
                   AND sensitivity = ?2 AND crisis_related = 0"
            }
            ContextItemType::Claim => {
                "SELECT payload_json FROM claims
                 WHERE id = ?1 AND status IN ('confirmed', 'revised', 'contradicted')
                   AND sensitivity = ?2 AND crisis_related = 0"
            }
            ContextItemType::Correction => {
                "SELECT payload_json FROM user_corrections
                 WHERE id = ?1 AND active = 1 AND sensitivity = ?2"
            }
            ContextItemType::OpenQuestion => {
                "SELECT payload_json FROM open_questions
                 WHERE id = ?1 AND status = 'open'
                   AND sensitivity = ?2 AND crisis_related = 0"
            }
            ContextItemType::Experiment => {
                "SELECT payload_json FROM experiments
                 WHERE id = ?1 AND status = 'active'
                   AND sensitivity = ?2 AND crisis_related = 0"
            }
        };
        let payload = transaction
            .query_row(
                query,
                params![item.item_id.to_string(), sensitivity],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(quisum_core::DomainError::InvalidValue(
                "context.item_eligibility",
            ))?;
        if item.item_type == ContextItemType::Claim {
            let claim: Claim = serde_json::from_str(&payload)?;
            claim.validate()?;
            if claim.kind == ClaimKind::CurrentState
                && claim
                    .revisit_after
                    .is_some_and(|revisit| revisit <= packet.created_at)
            {
                return Err(quisum_core::DomainError::InvalidValue("context.expired_claim").into());
            }
        }
    }
    Ok(())
}

fn update_context_packet(
    transaction: &Transaction<'_>,
    packet: &ContextPacket,
) -> Result<(), VaultError> {
    packet.validate()?;
    let changed = transaction.execute(
        "UPDATE context_packets
         SET approved_at = ?2, dispatched_at = ?3, payload_json = ?4
         WHERE id = ?1",
        params![
            packet.id.to_string(),
            packet.approved_at.map(format_time),
            packet.dispatched_at.map(format_time),
            serde_json::to_string(packet)?
        ],
    )?;
    if changed != 1 {
        return Err(VaultError::NotFoundRecord);
    }
    Ok(())
}

fn require_dispatched_model_context(
    transaction: &Transaction<'_>,
    source: &Source,
) -> Result<Option<ContextPacket>, VaultError> {
    if source.actor != SourceActor::Model {
        return Ok(None);
    }
    let packet_id =
        source
            .context_packet_id
            .as_ref()
            .ok_or(quisum_core::DomainError::InvalidValue(
                "source.context_packet_id",
            ))?;
    let packet = load_context_packet(transaction, packet_id)?;
    packet.authorize_egress()?;
    if packet.dispatched_at.is_none()
        || source.method.as_deref() != Some(packet.method.as_str())
        || source.model.as_deref() != Some(packet.model.as_str())
    {
        return Err(quisum_core::DomainError::InvalidValue("source.context_packet_id").into());
    }
    Ok(Some(packet))
}

fn require_dispatched_context_contains(
    transaction: &Transaction<'_>,
    packet_id: &EntityId,
    required_ids: &[&EntityId],
) -> Result<(), VaultError> {
    packet_id.require_kind(EntityKind::Context)?;
    let packet = load_context_packet(transaction, packet_id)?;
    packet.authorize_egress()?;
    if packet.dispatched_at.is_none()
        || required_ids.iter().any(|required_id| {
            !packet
                .items
                .iter()
                .any(|item| item.item_id.eq(*required_id))
        })
    {
        return Err(quisum_core::DomainError::InvalidValue("context.required_items").into());
    }
    Ok(())
}

fn load_experiment(
    transaction: &Transaction<'_>,
    experiment_id: &EntityId,
) -> Result<BehavioralExperiment, VaultError> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM experiments WHERE id = ?1",
            [experiment_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(VaultError::NotFoundRecord)?;
    let experiment: BehavioralExperiment = serde_json::from_str(&payload)?;
    experiment.validate()?;
    Ok(experiment)
}

fn update_experiment(
    transaction: &Transaction<'_>,
    experiment: &BehavioralExperiment,
) -> Result<(), VaultError> {
    experiment.validate()?;
    let changed = transaction.execute(
        "UPDATE experiments
         SET status = ?2, completed_at = ?3, payload_json = ?4
         WHERE id = ?1",
        params![
            experiment.id.to_string(),
            enum_token(&experiment.status)?,
            experiment.completed_at.map(format_time),
            serde_json::to_string(experiment)?
        ],
    )?;
    if changed != 1 {
        return Err(VaultError::NotFoundRecord);
    }
    Ok(())
}

fn insert_experiment_audit(
    transaction: &Transaction<'_>,
    experiment: &BehavioralExperiment,
    previous: ExperimentStatus,
    changed_at: DateTime<Utc>,
) -> Result<(), VaultError> {
    let audit = AuditEvent::new(
        ActorKind::User,
        AuditAction::ExperimentReviewed,
        Some(experiment.id.clone()),
        AuditDetails::Lifecycle {
            from: enum_token(&previous)?,
            to: enum_token(&experiment.status)?,
        },
        changed_at,
    );
    insert_audit(transaction, &audit)
}

fn insert_event_search_index(
    transaction: &Transaction<'_>,
    event: &Event,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO events_fts(id, title, facts, interpretations, topics)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            event.id.to_string(),
            event.title,
            event.facts.join("\n"),
            event.interpretations.join("\n"),
            event.topics.iter().cloned().collect::<Vec<_>>().join(" ")
        ],
    )?;
    Ok(())
}

const fn claim_is_searchable(status: ClaimStatus) -> bool {
    matches!(
        status,
        ClaimStatus::Confirmed | ClaimStatus::Revised | ClaimStatus::Contradicted
    )
}

fn insert_claim_search_index(
    transaction: &Transaction<'_>,
    claim: &Claim,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO claims_fts(id, statement, contexts, alternatives)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            claim.id.to_string(),
            claim.statement,
            claim.contexts.iter().cloned().collect::<Vec<_>>().join(" "),
            claim.alternative_explanations.join("\n")
        ],
    )?;
    Ok(())
}

fn insert_claim_projection(transaction: &Transaction<'_>, claim: &Claim) -> Result<(), VaultError> {
    let payload = serde_json::to_string(claim)?;
    transaction
        .execute(
            "INSERT INTO claims(
               id, kind, status, statement, evidence_grade, sensitivity,
               crisis_related, revisit_after, created_at, updated_at, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                claim.id.to_string(),
                enum_token(&claim.kind)?,
                enum_token(&claim.status)?,
                claim.statement,
                enum_token(&claim.evidence_grade)?,
                enum_token(&claim.sensitivity)?,
                i64::from(claim.crisis_related),
                claim.revisit_after.map(format_time),
                format_time(claim.created_at),
                format_time(claim.updated_at),
                payload
            ],
        )
        .map_err(map_constraint)?;
    if claim_is_searchable(claim.status) {
        insert_claim_search_index(transaction, claim)?;
    }
    Ok(())
}

fn update_claim_projection(transaction: &Transaction<'_>, claim: &Claim) -> Result<(), VaultError> {
    claim.validate()?;
    let changed = transaction.execute(
        "UPDATE claims
         SET kind = ?2, status = ?3, statement = ?4, evidence_grade = ?5,
             sensitivity = ?6, crisis_related = ?7, revisit_after = ?8,
             updated_at = ?9, payload_json = ?10
         WHERE id = ?1",
        params![
            claim.id.to_string(),
            enum_token(&claim.kind)?,
            enum_token(&claim.status)?,
            claim.statement,
            enum_token(&claim.evidence_grade)?,
            enum_token(&claim.sensitivity)?,
            i64::from(claim.crisis_related),
            claim.revisit_after.map(format_time),
            format_time(claim.updated_at),
            serde_json::to_string(claim)?
        ],
    )?;
    if changed != 1 {
        return Err(VaultError::NotFoundRecord);
    }
    transaction.execute(
        "DELETE FROM claims_fts WHERE id = ?1",
        [claim.id.to_string()],
    )?;
    if claim_is_searchable(claim.status) {
        insert_claim_search_index(transaction, claim)?;
    }
    Ok(())
}

fn load_claim(transaction: &Transaction<'_>, claim_id: &EntityId) -> Result<Claim, VaultError> {
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM claims WHERE id = ?1",
            [claim_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(VaultError::NotFoundRecord)?;
    let claim: Claim = serde_json::from_str(&payload)?;
    claim.validate()?;
    Ok(claim)
}

struct NewEvidenceLink<'a> {
    claim_id: &'a EntityId,
    evidence_id: &'a EntityId,
    relation: &'static str,
    review_status: &'static str,
    actor: ActorKind,
    context_packet_id: Option<&'a EntityId>,
    created_at: DateTime<Utc>,
}

fn insert_evidence_link(
    transaction: &Transaction<'_>,
    link: NewEvidenceLink<'_>,
) -> Result<(), VaultError> {
    transaction
        .execute(
            "INSERT INTO evidence_links(
               claim_id, evidence_id, relation, review_status,
               source_actor, context_packet_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                link.claim_id.to_string(),
                link.evidence_id.to_string(),
                link.relation,
                link.review_status,
                enum_token(&link.actor)?,
                link.context_packet_id.map(ToString::to_string),
                format_time(link.created_at)
            ],
        )
        .map_err(map_constraint)?;
    Ok(())
}

fn require_eligible_evidence_id(evidence_id: &EntityId) -> Result<(), VaultError> {
    if matches!(
        evidence_id.kind(),
        EntityKind::Event
            | EntityKind::Message
            | EntityKind::SessionSummary
            | EntityKind::Assessment
    ) {
        Ok(())
    } else {
        Err(quisum_core::DomainError::InvalidValue("claim.evidence_kind").into())
    }
}

fn require_eligible_evidence(
    transaction: &Transaction<'_>,
    evidence_id: &EntityId,
) -> Result<(), VaultError> {
    require_eligible_evidence_id(evidence_id)?;
    let eligible: bool = match evidence_id.kind() {
        EntityKind::Event => transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM events
               WHERE id = ?1 AND status = 'confirmed' AND crisis_related = 0
             )",
            [evidence_id.to_string()],
            |row| row.get(0),
        )?,
        EntityKind::Message => transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM messages WHERE id = ?1 AND crisis_related = 0
             )",
            [evidence_id.to_string()],
            |row| row.get(0),
        )?,
        EntityKind::SessionSummary => transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM session_summaries
               WHERE id = ?1 AND review_status = 'confirmed' AND crisis_related = 0
             )",
            [evidence_id.to_string()],
            |row| row.get(0),
        )?,
        EntityKind::Assessment => transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM assessments WHERE id = ?1 AND status = 'completed'
             )",
            [evidence_id.to_string()],
            |row| row.get(0),
        )?,
        EntityKind::Vault
        | EntityKind::Session
        | EntityKind::Claim
        | EntityKind::Revision
        | EntityKind::Review
        | EntityKind::Correction
        | EntityKind::OpenQuestion
        | EntityKind::Experiment
        | EntityKind::Context
        | EntityKind::Provider
        | EntityKind::Consent
        | EntityKind::Audit
        | EntityKind::Export
        | EntityKind::Backup
        | EntityKind::Import => false,
    };
    if eligible {
        Ok(())
    } else {
        Err(quisum_core::DomainError::InvalidValue("claim.evidence_eligibility").into())
    }
}

fn insert_review(transaction: &Transaction<'_>, review: &UserReview) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO user_reviews(id, claim_id, decision, reviewed_at, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            review.id.to_string(),
            review.claim_id.to_string(),
            enum_token(&review.decision)?,
            format_time(review.reviewed_at),
            serde_json::to_string(review)?
        ],
    )?;
    Ok(())
}

fn insert_revision(
    transaction: &Transaction<'_>,
    revision: &ClaimRevision,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO claim_revisions(
           id, claim_id, kind, actor, reason, before_json, after_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            revision.id.to_string(),
            revision.claim_id.to_string(),
            enum_token(&revision.kind)?,
            enum_token(&revision.actor)?,
            revision.reason,
            serde_json::to_string(&revision.before)?,
            serde_json::to_string(&revision.after)?,
            format_time(revision.created_at)
        ],
    )?;
    Ok(())
}

fn insert_open_question(
    transaction: &Transaction<'_>,
    question: &OpenQuestion,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO open_questions(
           id, status, sensitivity, crisis_related, created_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            question.id.to_string(),
            enum_token(&question.status)?,
            enum_token(&question.sensitivity)?,
            i64::from(question.crisis_related),
            format_time(question.created_at),
            serde_json::to_string(question)?
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_audit(
    transaction: &Transaction<'_>,
    audit: &AuditEvent,
) -> Result<(), VaultError> {
    audit.validate()?;
    let entity_type = audit.entity_type.as_ref().map(enum_token).transpose()?;
    transaction.execute(
        "INSERT INTO audit_events(
           id, actor, action, entity_type, entity_id, details_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            audit.id.to_string(),
            enum_token(&audit.actor)?,
            enum_token(&audit.action)?,
            entity_type,
            audit.entity_id.as_ref().map(ToString::to_string),
            serde_json::to_string(&audit.details)?,
            format_time(audit.created_at)
        ],
    )?;
    Ok(())
}

fn enum_token(value: &impl Serialize) -> Result<String, VaultError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(token) => Ok(token),
        _ => Err(quisum_core::DomainError::InvalidValue("database.enum_token").into()),
    }
}

fn format_time(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn map_constraint(error: rusqlite::Error) -> VaultError {
    match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == ErrorCode::ConstraintViolation =>
        {
            VaultError::Conflict
        }
        _ => VaultError::Database(error),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use chrono::Utc;
    use quisum_core::{
        ClaimKind, ContextBuilder, ContextItemType, EvidenceGrade, ExperimentRisk, MemoryCandidate,
        MemoryState, ReviewDecision, Sensitivity, Source, UserEndorsement,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::VaultKey;

    fn test_vault() -> Result<(TempDir, VaultKey, SqlCipherVault), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(directory.path().join("vault.db"), &key)?;
        Ok((directory, key, vault))
    }

    fn insert_confirmed_event(
        vault: &SqlCipherVault,
        title: &str,
    ) -> Result<Event, Box<dyn Error>> {
        let event = Event::user_confirmed(
            title,
            vec!["A synthetic observable fact.".to_owned()],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;
        Ok(event)
    }

    fn model_source(vault: &SqlCipherVault, method: &str) -> Result<Source, Box<dyn Error>> {
        model_source_with_candidates(vault, method, Vec::new())
    }

    fn model_source_with_candidates(
        vault: &SqlCipherVault,
        method: &str,
        candidates: Vec<MemoryCandidate>,
    ) -> Result<Source, Box<dyn Error>> {
        let packet = ContextBuilder::new(1)?.build(
            Actor::model_host(),
            "Synthetic test generation",
            method,
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "Synthetic test query",
            candidates,
            Utc::now(),
        )?;
        vault.insert_context_packet(Actor::system(), &packet)?;
        vault.mark_context_packet_dispatched(Actor::system(), &packet.id, Utc::now())?;
        Ok(Source::model(
            method,
            "synthetic-provider",
            "synthetic-model",
            packet.id,
        ))
    }

    fn pending_pattern(
        vault: &SqlCipherVault,
        event_id: EntityId,
    ) -> Result<Claim, Box<dyn Error>> {
        let mut claim = Claim::proposal(
            ClaimKind::PatternHypothesis,
            "A narrow synthetic pattern may apply in one context.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            model_source_with_candidates(
                vault,
                "pattern-validation@1.0.0",
                vec![MemoryCandidate {
                    item_type: ContextItemType::Event,
                    item_id: event_id.clone(),
                    excerpt: "Synthetic evidence excerpt.".to_owned(),
                    sensitivity: Sensitivity::Private,
                    state: MemoryState::Confirmed,
                    crisis_related: false,
                    relevant: true,
                    selection_reason: "Synthetic direct evidence.".to_owned(),
                    relevance_score: 100,
                    recorded_at: Utc::now(),
                }],
            )?,
            Utc::now(),
        );
        claim.supporting_evidence.insert(event_id);
        claim
            .alternative_explanations
            .push("A temporary constraint could explain the event.".to_owned());
        Ok(claim)
    }

    fn confirm_claim(vault: &SqlCipherVault, claim_id: &EntityId) -> Result<(), Box<dyn Error>> {
        vault.review_claim(
            Actor::user(),
            claim_id,
            ReviewRequest {
                decision: ReviewDecision::Confirm,
                edited_statement: None,
                note: None,
            },
            Utc::now(),
        )?;
        Ok(())
    }

    #[test]
    fn claim_review_round_trips_with_revision_and_audit() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = insert_confirmed_event(&vault, "Synthetic meeting")?;
        let claim = pending_pattern(&vault, event.id)?;
        let claim_id = claim.id.clone();
        vault.insert_claim(Actor::model_host(), &claim)?;
        assert_eq!(vault.list_review_claims(Actor::user(), 10)?.len(), 1);
        {
            let connection = vault.connection.lock();
            let indexed: i64 = connection.query_row(
                "SELECT COUNT(*) FROM claims_fts WHERE id = ?1",
                [claim_id.to_string()],
                |row| row.get(0),
            )?;
            assert_eq!(indexed, 0);
        }

        vault.review_claim(
            Actor::user(),
            &claim_id,
            ReviewRequest {
                decision: ReviewDecision::ReviseAndConfirm,
                edited_statement: Some(
                    "A narrower synthetic pattern may apply at work.".to_owned(),
                ),
                note: Some("Synthetic user note.".to_owned()),
            },
            Utc::now(),
        )?;
        let stored = vault
            .get_claim(Actor::user(), &claim_id)?
            .ok_or("claim not found")?;
        assert_eq!(stored.status, ClaimStatus::Revised);
        assert_eq!(stored.user_endorsement, Some(UserEndorsement::Edited));
        assert!(vault.get_claim(Actor::model_host(), &claim_id)?.is_some());
        {
            let connection = vault.connection.lock();
            let indexed: i64 = connection.query_row(
                "SELECT COUNT(*) FROM claims_fts WHERE id = ?1",
                [claim_id.to_string()],
                |row| row.get(0),
            )?;
            assert_eq!(indexed, 1);
        }
        assert_eq!(vault.claim_revision_count(&claim_id)?, 1);
        assert!(vault.audit_count()? >= 4);
        Ok(())
    }

    #[test]
    fn model_host_cannot_insert_or_review_confirmed_claim() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = insert_confirmed_event(&vault, "Synthetic event")?;
        let mut claim = pending_pattern(&vault, event.id)?;
        claim.status = ClaimStatus::Confirmed;
        claim.user_endorsement = Some(UserEndorsement::Accepted);
        assert!(vault.insert_claim(Actor::model_host(), &claim).is_err());

        claim.status = ClaimStatus::PendingReview;
        claim.user_endorsement = None;
        vault.insert_claim(Actor::model_host(), &claim)?;
        let mut unsafe_claim = claim.clone();
        unsafe_claim.id = EntityId::new(EntityKind::Claim);
        unsafe_claim.statement = "This proves you have depression.".to_owned();
        assert!(matches!(
            vault.insert_claim(Actor::model_host(), &unsafe_claim),
            Err(VaultError::Domain(quisum_core::DomainError::SafetyBlocked))
        ));
        assert!(
            vault
                .review_claim(
                    Actor::model_host(),
                    &claim.id,
                    ReviewRequest {
                        decision: ReviewDecision::Confirm,
                        edited_statement: None,
                        note: None,
                    },
                    Utc::now(),
                )
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn counterevidence_is_only_applied_after_user_acceptance() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let support = insert_confirmed_event(&vault, "Supporting event")?;
        let counter = insert_confirmed_event(&vault, "Counterexample")?;
        let claim = pending_pattern(&vault, support.id)?;
        let claim_id = claim.id.clone();
        vault.insert_claim(Actor::model_host(), &claim)?;
        confirm_claim(&vault, &claim_id)?;

        let now = Utc::now();
        assert!(
            vault
                .propose_counterevidence(Actor::model_host(), &claim_id, &counter.id, None, now,)
                .is_err()
        );
        let packet = ContextBuilder::new(2)?.build(
            Actor::model_host(),
            "Synthetic counterevidence review",
            "pattern-validation@1.0.0",
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "Find a bounded counterexample.",
            vec![
                MemoryCandidate {
                    item_type: ContextItemType::Claim,
                    item_id: claim_id.clone(),
                    excerpt: "A synthetic confirmed claim.".to_owned(),
                    sensitivity: Sensitivity::Private,
                    state: MemoryState::Confirmed,
                    crisis_related: false,
                    relevant: true,
                    selection_reason: "The claim under review.".to_owned(),
                    relevance_score: 100,
                    recorded_at: now,
                },
                MemoryCandidate {
                    item_type: ContextItemType::Event,
                    item_id: counter.id.clone(),
                    excerpt: "A synthetic counterexample.".to_owned(),
                    sensitivity: Sensitivity::Private,
                    state: MemoryState::Confirmed,
                    crisis_related: false,
                    relevant: true,
                    selection_reason: "Potential counterevidence.".to_owned(),
                    relevance_score: 90,
                    recorded_at: now,
                },
            ],
            now,
        )?;
        vault.insert_context_packet(Actor::system(), &packet)?;
        vault.mark_context_packet_dispatched(Actor::system(), &packet.id, Utc::now())?;
        vault.propose_counterevidence(
            Actor::model_host(),
            &claim_id,
            &counter.id,
            Some(&packet.id),
            Utc::now(),
        )?;
        let before = vault
            .get_claim(Actor::user(), &claim_id)?
            .ok_or("claim not found")?;
        assert_eq!(before.status, ClaimStatus::Confirmed);
        assert!(!before.counterevidence.contains(&counter.id));

        let after =
            vault.accept_counterevidence(Actor::user(), &claim_id, &counter.id, Utc::now())?;
        assert_eq!(after.status, ClaimStatus::Contradicted);
        assert!(after.counterevidence.contains(&counter.id));
        Ok(())
    }

    #[test]
    fn deleting_evidence_repairs_confirmed_and_purges_unreviewed_claims()
    -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = insert_confirmed_event(&vault, "Shared supporting event")?;
        let confirmed = pending_pattern(&vault, event.id.clone())?;
        let confirmed_id = confirmed.id.clone();
        vault.insert_claim(Actor::model_host(), &confirmed)?;
        confirm_claim(&vault, &confirmed_id)?;

        let pending = pending_pattern(&vault, event.id.clone())?;
        let pending_id = pending.id.clone();
        vault.insert_claim(Actor::model_host(), &pending)?;

        let impact = vault.delete_event(Actor::user(), &event.id, Utc::now())?;
        assert_eq!(impact.affected_claims, vec![confirmed_id.clone()]);
        assert_eq!(impact.purged_pending_claims, vec![pending_id.clone()]);
        let repaired = vault
            .get_claim(Actor::user(), &confirmed_id)?
            .ok_or("confirmed claim not found")?;
        assert_eq!(repaired.status, ClaimStatus::Contradicted);
        assert!(repaired.supporting_evidence.is_empty());
        assert!(vault.get_claim(Actor::user(), &pending_id)?.is_none());
        Ok(())
    }

    #[test]
    fn deleting_last_support_preserves_a_user_uncertain_claim() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = insert_confirmed_event(&vault, "Uncertain claim evidence")?;
        let claim = pending_pattern(&vault, event.id.clone())?;
        let claim_id = claim.id.clone();
        vault.insert_claim(Actor::model_host(), &claim)?;
        vault.review_claim(
            Actor::user(),
            &claim_id,
            ReviewRequest {
                decision: ReviewDecision::Uncertain,
                edited_statement: None,
                note: None,
            },
            Utc::now(),
        )?;

        vault.delete_event(Actor::user(), &event.id, Utc::now())?;
        let retained = vault
            .get_claim(Actor::user(), &claim_id)?
            .ok_or("uncertain claim was unexpectedly purged")?;
        assert_eq!(retained.status, ClaimStatus::Uncertain);
        assert_eq!(retained.evidence_grade, EvidenceGrade::Contested);
        assert!(retained.supporting_evidence.is_empty());
        Ok(())
    }

    #[test]
    fn append_only_audit_trigger_rejects_mutation() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let connection = vault.connection.lock();
        let result = connection.execute("UPDATE audit_events SET action = 'policy_changed'", []);
        assert!(result.is_err());
        let result = connection.execute("DELETE FROM audit_events", []);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn model_event_requires_user_review_and_crisis_candidates_are_blocked()
    -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let source = model_source(&vault, "event-debrief@1.0.0")?;
        let mut event = Event::proposed(
            "Synthetic proposed event",
            vec!["A synthetic observable fact.".to_owned()],
            Utc::now(),
            Sensitivity::Private,
            source,
        );
        vault.insert_event(Actor::model_host(), &event)?;
        {
            let connection = vault.connection.lock();
            let indexed: i64 = connection.query_row(
                "SELECT COUNT(*) FROM events_fts WHERE id = ?1",
                [event.id.to_string()],
                |row| row.get(0),
            )?;
            assert_eq!(indexed, 0);
        }
        let reviewed = vault.review_event(Actor::user(), &event.id, true, Utc::now())?;
        assert_eq!(reviewed.status, EventStatus::Confirmed);
        {
            let connection = vault.connection.lock();
            let indexed: i64 = connection.query_row(
                "SELECT COUNT(*) FROM events_fts WHERE id = ?1",
                [event.id.to_string()],
                |row| row.get(0),
            )?;
            assert_eq!(indexed, 1);
        }

        event.id = EntityId::new(EntityKind::Event);
        event.crisis_related = true;
        assert!(matches!(
            vault.insert_event(Actor::model_host(), &event),
            Err(VaultError::Domain(quisum_core::DomainError::SafetyBlocked))
        ));
        Ok(())
    }

    #[test]
    fn proposed_event_is_not_eligible_claim_evidence() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = Event::proposed(
            "Unreviewed synthetic event",
            vec!["An unreviewed synthetic fact.".to_owned()],
            Utc::now(),
            Sensitivity::Private,
            model_source(&vault, "event-debrief@1.0.0")?,
        );
        vault.insert_event(Actor::model_host(), &event)?;
        let mut claim = Claim::proposal(
            ClaimKind::PatternHypothesis,
            "An unreviewed event must not support a pattern.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            model_source(&vault, "pattern-validation@1.0.0")?,
            Utc::now(),
        );
        claim.supporting_evidence.insert(event.id);
        claim
            .alternative_explanations
            .push("The source event is still unreviewed.".to_owned());
        let claim_id = claim.id.clone();
        assert!(vault.insert_claim(Actor::model_host(), &claim).is_err());
        assert!(vault.get_claim(Actor::user(), &claim_id)?.is_none());
        Ok(())
    }

    #[test]
    fn failed_review_rolls_back_projection_revision_and_audit() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = insert_confirmed_event(&vault, "Synthetic rollback event")?;
        let claim = pending_pattern(&vault, event.id)?;
        let claim_id = claim.id.clone();
        vault.insert_claim(Actor::model_host(), &claim)?;
        let audit_before = vault.audit_count()?;
        {
            let connection = vault.connection.lock();
            connection.execute_batch(
                "CREATE TRIGGER synthetic_fail_review
                 BEFORE INSERT ON user_reviews
                 BEGIN
                   SELECT RAISE(ABORT, 'synthetic review failure');
                 END;",
            )?;
        }

        assert!(
            vault
                .review_claim(
                    Actor::user(),
                    &claim_id,
                    ReviewRequest {
                        decision: ReviewDecision::Confirm,
                        edited_statement: None,
                        note: None,
                    },
                    Utc::now(),
                )
                .is_err()
        );
        let stored = vault
            .get_claim(Actor::user(), &claim_id)?
            .ok_or("claim disappeared after rollback")?;
        assert_eq!(stored.status, ClaimStatus::PendingReview);
        assert_eq!(vault.claim_revision_count(&claim_id)?, 0);
        assert_eq!(vault.audit_count()?, audit_before);
        Ok(())
    }

    #[test]
    fn model_candidate_requires_a_dispatched_matching_context() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let event = Event::proposed(
            "Synthetic ungrounded event",
            vec!["A synthetic observable fact.".to_owned()],
            Utc::now(),
            Sensitivity::Private,
            Source::model(
                "event-debrief@1.0.0",
                "synthetic-provider",
                "synthetic-model",
                EntityId::new(EntityKind::Context),
            ),
        );
        assert!(matches!(
            vault.insert_event(Actor::model_host(), &event),
            Err(VaultError::NotFoundRecord)
        ));

        let mut mismatched = event;
        mismatched.id = EntityId::new(EntityKind::Event);
        mismatched.source = model_source(&vault, "pattern-validation@1.0.0")?;
        mismatched.source.method = Some("event-debrief@1.0.0".to_owned());
        assert!(
            vault
                .insert_event(Actor::model_host(), &mismatched)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn highly_sensitive_context_needs_one_time_user_approval() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let now = Utc::now();
        let event = Event::user_confirmed(
            "Synthetic sensitive source",
            vec!["A synthetic highly sensitive fact.".to_owned()],
            None,
            now,
            Sensitivity::HighlySensitive,
        );
        vault.insert_event(Actor::user(), &event)?;
        let packet = ContextBuilder::new(1)?.build(
            Actor::model_host(),
            "Synthetic sensitive test",
            "free-reflection@1.0.0",
            EntityId::new(EntityKind::Provider),
            "synthetic-model",
            "Synthetic query",
            vec![MemoryCandidate {
                item_type: ContextItemType::Event,
                item_id: event.id,
                excerpt: "Synthetic highly sensitive excerpt.".to_owned(),
                sensitivity: Sensitivity::HighlySensitive,
                state: MemoryState::Confirmed,
                crisis_related: false,
                relevant: true,
                selection_reason: "Synthetic direct relevance.".to_owned(),
                relevance_score: 100,
                recorded_at: now,
            }],
            now,
        )?;
        vault.insert_context_packet(Actor::system(), &packet)?;
        assert!(
            vault
                .mark_context_packet_dispatched(Actor::system(), &packet.id, Utc::now())
                .is_err()
        );
        let approved = vault.approve_context_packet(Actor::user(), &packet.id, Utc::now())?;
        assert!(approved.approved_at.is_some());
        assert!(
            vault
                .approve_context_packet(Actor::user(), &packet.id, Utc::now())
                .is_err()
        );
        let dispatched =
            vault.mark_context_packet_dispatched(Actor::system(), &packet.id, Utc::now())?;
        assert!(dispatched.dispatched_at.is_some());
        Ok(())
    }

    #[test]
    fn experiment_lifecycle_requires_user_review() -> Result<(), Box<dyn Error>> {
        let (_directory, _key, vault) = test_vault()?;
        let planned_at = Utc::now();
        let experiment = BehavioralExperiment::proposal(
            "Synthetic reversible experiment",
            "A contextual hypothesis.",
            "One observable prediction.",
            "Try one reversible action once.",
            "Stop if consequences become unclear.",
            ExperimentRisk::Low,
            planned_at,
            Sensitivity::Private,
            Source::user(),
        );
        let experiment_id = experiment.id.clone();
        vault.insert_experiment(Actor::user(), &experiment)?;
        assert!(
            vault
                .get_experiment(Actor::model_host(), &experiment_id)?
                .is_none()
        );
        assert!(
            vault
                .activate_experiment(Actor::model_host(), &experiment_id, Utc::now())
                .is_err()
        );

        let active = vault.activate_experiment(Actor::user(), &experiment_id, Utc::now())?;
        assert_eq!(active.status, ExperimentStatus::Active);
        assert!(
            vault
                .get_experiment(Actor::model_host(), &experiment_id)?
                .is_some()
        );
        let completed = vault.complete_experiment(
            Actor::user(),
            &experiment_id,
            "The observable result was mixed.",
            "The hypothesis needs a narrower context.",
            Utc::now(),
        )?;
        assert_eq!(completed.status, ExperimentStatus::Completed);
        assert_eq!(
            completed.actual_result.as_deref(),
            Some("The observable result was mixed.")
        );
        Ok(())
    }
}
