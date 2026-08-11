use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use quisum_core::{
    Actor, ActorKind, AuditAction, AuditDetails, AuditEvent, Capability, Claim, ClaimStatus,
    ContextItemType, EntityId, EntityKind, MessageRole, ProviderProfile, ReflectionSession,
    SafetyEngine, Sensitivity, SessionMessage, SessionStatus, SourceActor, Validate,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{SqlCipherVault, VaultError, store};

/// Result of deleting a session transcript and repairing dependent claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDeletionImpact {
    /// Deleted session.
    pub session_id: EntityId,
    /// Number of transcript messages removed.
    pub deleted_messages: u64,
    /// Retained claims conservatively revised after source removal.
    pub affected_claims: Vec<EntityId>,
    /// Unreviewed claims with no remaining support that were purged.
    pub purged_pending_claims: Vec<EntityId>,
}

impl SqlCipherVault {
    /// Create a user-owned active reflection session.
    pub fn create_session(
        &self,
        actor: Actor,
        session: &ReflectionSession,
    ) -> Result<(), VaultError> {
        require_user_session_actor(actor)?;
        session.validate()?;
        if session.status != SessionStatus::Active || session.source.actor != SourceActor::User {
            return Err(quisum_core::DomainError::InvalidValue("session.initial_state").into());
        }
        if let Some(title) = session.title.as_deref() {
            store::require_safe_user_input(title)?;
        }
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction
            .execute(
                "INSERT INTO sessions(
                   id, method, status, started_at, completed_at,
                   sensitivity, crisis_related, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, 0, ?6)",
                params![
                    session.id.to_string(),
                    session.method,
                    store::enum_token(&session.status)?,
                    store::format_time(session.started_at),
                    store::enum_token(&session.sensitivity)?,
                    serde_json::to_string(session)?
                ],
            )
            .map_err(store::map_constraint)?;
        store::insert_audit(
            &transaction,
            &AuditEvent::new(
                actor.kind(),
                AuditAction::SessionCreated,
                Some(session.id.clone()),
                AuditDetails::None,
                session.started_at,
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Append direct user text or a locally validated provider response to an active session.
    pub fn append_session_message(
        &self,
        actor: Actor,
        message: &SessionMessage,
    ) -> Result<(), VaultError> {
        require_user_session_actor(actor)?;
        message.validate()?;
        let safety = match message.role {
            MessageRole::User => SafetyEngine.screen_user_input(&message.content),
            MessageRole::Assistant => SafetyEngine.review_generated_text(&message.content),
        };
        SafetyEngine.require_persistence_allowed(&safety)?;

        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let session = load_session(&transaction, &message.session_id)?;
        if session.status != SessionStatus::Active
            || message.created_at < session.started_at
            || message.sensitivity < session.sensitivity
        {
            return Err(quisum_core::DomainError::InvalidValue("message.session_state").into());
        }
        let preceding_message = transaction
            .query_row(
                "SELECT payload_json FROM messages
                 WHERE session_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                [session.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str::<SessionMessage>(&payload))
            .transpose()?;
        if preceding_message
            .as_ref()
            .is_some_and(|preceding| message.created_at <= preceding.created_at)
        {
            return Err(quisum_core::DomainError::InvalidValue("message.created_at").into());
        }
        if message.role == MessageRole::Assistant {
            require_assistant_provenance(
                &transaction,
                &session,
                preceding_message.as_ref(),
                message,
            )?;
        }

        transaction
            .execute(
                "INSERT INTO messages(
                   id, session_id, role, created_at, sensitivity,
                   crisis_related, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
                params![
                    message.id.to_string(),
                    message.session_id.to_string(),
                    store::enum_token(&message.role)?,
                    store::format_time(message.created_at),
                    store::enum_token(&message.sensitivity)?,
                    serde_json::to_string(message)?
                ],
            )
            .map_err(store::map_constraint)?;
        if message.role == MessageRole::User {
            crate::retrieval::upsert_embedding(
                &transaction,
                &message.id,
                ContextItemType::Message,
                &message.content,
                message.created_at,
            )?;
        }
        store::insert_audit(
            &transaction,
            &AuditEvent::new(
                actor.kind(),
                AuditAction::MessageRecorded,
                Some(message.id.clone()),
                AuditDetails::None,
                message.created_at,
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Complete or abandon an active session.
    pub fn close_session(
        &self,
        actor: Actor,
        session_id: &EntityId,
        completed: bool,
        closed_at: DateTime<Utc>,
    ) -> Result<ReflectionSession, VaultError> {
        require_user_session_actor(actor)?;
        session_id.require_kind(EntityKind::Session)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut session = load_session(&transaction, session_id)?;
        let previous = session.status;
        session.close(actor, completed, closed_at)?;
        let changed = transaction.execute(
            "UPDATE sessions
             SET status = ?2, completed_at = ?3, payload_json = ?4
             WHERE id = ?1 AND status = 'active'",
            params![
                session.id.to_string(),
                store::enum_token(&session.status)?,
                session.completed_at.map(store::format_time),
                serde_json::to_string(&session)?
            ],
        )?;
        if changed != 1 {
            return Err(VaultError::Conflict);
        }
        store::insert_audit(
            &transaction,
            &AuditEvent::new(
                actor.kind(),
                AuditAction::SessionCompleted,
                Some(session.id.clone()),
                AuditDetails::Lifecycle {
                    from: store::enum_token(&previous)?,
                    to: store::enum_token(&session.status)?,
                },
                closed_at,
            ),
        )?;
        transaction.commit()?;
        Ok(session)
    }

    /// Delete one session transcript and its local retrieval vectors.
    pub fn delete_session(
        &self,
        actor: Actor,
        session_id: &EntityId,
        deleted_at: DateTime<Utc>,
    ) -> Result<SessionDeletionImpact, VaultError> {
        actor.require(Capability::DeleteData)?;
        if actor.kind() != ActorKind::User {
            return Err(quisum_core::DomainError::Forbidden(Capability::DeleteData).into());
        }
        session_id.require_kind(EntityKind::Session)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let session = load_session(&transaction, session_id)?;
        if deleted_at < session.started_at {
            return Err(quisum_core::DomainError::InvalidValue("session.deleted_at").into());
        }
        let message_ids = {
            let mut statement =
                transaction.prepare("SELECT id FROM messages WHERE session_id = ?1 ORDER BY id")?;
            let rows =
                statement.query_map([session_id.to_string()], |row| row.get::<_, String>(0))?;
            let mut identifiers = Vec::new();
            for row in rows {
                identifiers.push(row?.parse::<EntityId>()?);
            }
            identifiers
        };
        for identifier in &message_ids {
            crate::retrieval::remove_embedding(&transaction, identifier)?;
        }
        let deleting = message_ids.iter().cloned().collect::<BTreeSet<_>>();
        let mut dependent_claims = BTreeMap::new();
        for identifier in &message_ids {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT c.id, c.payload_json
                 FROM claims c
                 JOIN evidence_links e ON e.claim_id = c.id
                 WHERE e.evidence_id = ?1",
            )?;
            let rows = statement.query_map([identifier.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (claim_id, payload) = row?;
                dependent_claims.entry(claim_id).or_insert(payload);
            }
        }
        let mut affected_claims = Vec::new();
        let mut purged_pending_claims = Vec::new();
        for payload in dependent_claims.into_values() {
            let mut claim: Claim = serde_json::from_str(&payload)?;
            claim.validate()?;
            let remaining_support = claim
                .supporting_evidence
                .difference(&deleting)
                .next()
                .is_some();
            let deletes_support = claim
                .supporting_evidence
                .intersection(&deleting)
                .next()
                .is_some();
            if matches!(
                claim.status,
                ClaimStatus::Proposed | ClaimStatus::PendingReview
            ) && deletes_support
                && !remaining_support
            {
                transaction.execute("DELETE FROM claims WHERE id = ?1", [claim.id.to_string()])?;
                transaction.execute(
                    "DELETE FROM claims_fts WHERE id = ?1",
                    [claim.id.to_string()],
                )?;
                crate::retrieval::remove_embedding(&transaction, &claim.id)?;
                purged_pending_claims.push(claim.id);
                continue;
            }
            let mut revised = false;
            for identifier in &message_ids {
                if let Some(revision) = claim.remove_evidence(actor, identifier, deleted_at)? {
                    store::insert_revision(&transaction, &revision)?;
                    revised = true;
                }
            }
            if revised {
                store::update_claim_projection(&transaction, &claim)?;
                affected_claims.push(claim.id);
            }
        }
        for identifier in &message_ids {
            transaction.execute(
                "DELETE FROM evidence_links WHERE evidence_id = ?1",
                [identifier.to_string()],
            )?;
        }
        let changed = transaction.execute(
            "DELETE FROM sessions WHERE id = ?1",
            [session_id.to_string()],
        )?;
        if changed != 1 {
            return Err(VaultError::NotFoundRecord);
        }
        let deleted_messages = u64::try_from(message_ids.len())
            .map_err(|_| quisum_core::DomainError::InvalidValue("session.message_count"))?;
        let affected_count = message_ids
            .len()
            .saturating_add(affected_claims.len())
            .saturating_add(purged_pending_claims.len());
        store::insert_audit(
            &transaction,
            &AuditEvent::new(
                actor.kind(),
                AuditAction::EntityDeleted,
                Some(session_id.clone()),
                AuditDetails::AffectedRecords {
                    count: u64::try_from(affected_count).map_err(|_| {
                        quisum_core::DomainError::InvalidValue("session.affected_count")
                    })?,
                },
                deleted_at,
            ),
        )?;
        transaction.commit()?;
        affected_claims.sort();
        purged_pending_claims.sort();
        Ok(SessionDeletionImpact {
            session_id: session_id.clone(),
            deleted_messages,
            affected_claims,
            purged_pending_claims,
        })
    }

    /// Read one complete session for a local user surface.
    pub fn get_session(
        &self,
        actor: Actor,
        session_id: &EntityId,
    ) -> Result<Option<ReflectionSession>, VaultError> {
        require_user_session_actor(actor)?;
        session_id.require_kind(EntityKind::Session)?;
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT payload_json FROM sessions WHERE id = ?1",
                [session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| {
                let session: ReflectionSession = serde_json::from_str(&payload)?;
                session.validate()?;
                Ok(session)
            })
            .transpose()
    }

    /// List recent sessions for local Explore and CLI surfaces.
    pub fn list_sessions(
        &self,
        actor: Actor,
        limit: usize,
    ) -> Result<Vec<ReflectionSession>, VaultError> {
        require_user_session_actor(actor)?;
        if limit == 0 || limit > 500 {
            return Err(quisum_core::DomainError::InvalidValue("session.list_limit").into());
        }
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare("SELECT payload_json FROM sessions ORDER BY started_at DESC, id LIMIT ?1")?;
        let rows = statement.query_map(
            [i64::try_from(limit)
                .map_err(|_| quisum_core::DomainError::InvalidValue("session.list_limit"))?],
            |row| row.get::<_, String>(0),
        )?;
        let mut sessions = Vec::new();
        for row in rows {
            let session: ReflectionSession = serde_json::from_str(&row?)?;
            session.validate()?;
            sessions.push(session);
        }
        Ok(sessions)
    }

    /// List the bounded transcript for one session.
    pub fn list_session_messages(
        &self,
        actor: Actor,
        session_id: &EntityId,
        limit: usize,
    ) -> Result<Vec<SessionMessage>, VaultError> {
        require_user_session_actor(actor)?;
        session_id.require_kind(EntityKind::Session)?;
        if limit == 0 || limit > 2_000 {
            return Err(quisum_core::DomainError::InvalidValue("message.list_limit").into());
        }
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT payload_json FROM messages
             WHERE session_id = ?1 ORDER BY created_at, id LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![
                session_id.to_string(),
                i64::try_from(limit)
                    .map_err(|_| quisum_core::DomainError::InvalidValue("message.list_limit"))?
            ],
            |row| row.get::<_, String>(0),
        )?;
        let mut messages = Vec::new();
        for row in rows {
            let message: SessionMessage = serde_json::from_str(&row?)?;
            message.validate()?;
            messages.push(message);
        }
        Ok(messages)
    }
}

fn require_user_session_actor(actor: Actor) -> Result<(), VaultError> {
    actor.require(Capability::ManageSession)?;
    if actor.kind() == ActorKind::User {
        Ok(())
    } else {
        Err(quisum_core::DomainError::Forbidden(Capability::ManageSession).into())
    }
}

fn load_session(
    transaction: &Transaction<'_>,
    session_id: &EntityId,
) -> Result<ReflectionSession, VaultError> {
    session_id.require_kind(EntityKind::Session)?;
    let payload = transaction
        .query_row(
            "SELECT payload_json FROM sessions WHERE id = ?1",
            [session_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(VaultError::NotFoundRecord)?;
    let session: ReflectionSession = serde_json::from_str(&payload)?;
    session.validate()?;
    Ok(session)
}

fn require_assistant_provenance(
    transaction: &Transaction<'_>,
    session: &ReflectionSession,
    preceding_message: Option<&SessionMessage>,
    message: &SessionMessage,
) -> Result<(), VaultError> {
    let packet = store::require_dispatched_model_context(transaction, &message.source)?
        .ok_or(quisum_core::DomainError::InvalidValue("message.source"))?;
    if packet.method != session.method {
        return Err(quisum_core::DomainError::InvalidValue("message.method").into());
    }
    let required_sensitivity = packet
        .items
        .iter()
        .map(|item| item.sensitivity)
        .fold(packet.query_sensitivity, Sensitivity::max);
    if message.sensitivity < required_sensitivity {
        return Err(quisum_core::DomainError::InvalidValue("message.sensitivity").into());
    }
    let preceding_message =
        preceding_message.ok_or(quisum_core::DomainError::InvalidValue("message.user_turn"))?;
    if preceding_message.role != MessageRole::User || preceding_message.content != packet.query {
        return Err(quisum_core::DomainError::InvalidValue("message.user_turn").into());
    }
    let profile_payload: String = transaction
        .query_row(
            "SELECT payload_json FROM provider_profiles WHERE id = ?1",
            [packet.provider_profile_id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(VaultError::NotFoundRecord)?;
    let profile: ProviderProfile = serde_json::from_str(&profile_payload)?;
    profile.validate()?;
    if message.source.provider.as_deref() != Some(profile.name.as_str()) {
        return Err(quisum_core::DomainError::InvalidValue("message.provider").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, error::Error};

    use quisum_core::{
        ClaimKind, ContextBuilder, EvidenceGrade, MemoryCandidate, MemoryState, PrivacyKind,
        ProviderCapabilities, ProviderKind, ReviewDecision, ReviewRequest, Sensitivity, Source,
    };
    use tempfile::tempdir;

    use super::*;
    use crate::VaultKey;

    #[test]
    fn transcript_requires_active_session_and_verified_provider_provenance()
    -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let now = Utc::now();
        let session = ReflectionSession::user_started(
            "free-reflection@1.0.0",
            Some("Synthetic reflection".to_owned()),
            Sensitivity::Private,
            now,
        );
        vault.create_session(Actor::user(), &session)?;
        let user_message = SessionMessage::user(
            session.id.clone(),
            "A synthetic question",
            Sensitivity::Private,
            now,
        );
        vault.append_session_message(Actor::user(), &user_message)?;

        let mut profile = ProviderProfile::new(
            "Synthetic local",
            ProviderKind::LocalHttp,
            "http://127.0.0.1:11434/v1",
            "synthetic-model",
            PrivacyKind::Local,
            ProviderCapabilities {
                streaming: false,
                tool_use: false,
                json_schema: false,
                vision: false,
                context_limit: Some(8_192),
            },
            BTreeSet::from([Sensitivity::Private]),
            None,
            None,
            now,
        );
        vault.put_provider_profile(Actor::user(), &profile)?;
        let packet = ContextBuilder::new(4)?.build(
            Actor::user(),
            "Synthetic purpose",
            session.method.clone(),
            profile.id.clone(),
            profile.model.clone(),
            user_message.content.clone(),
            Sensitivity::Private,
            vec![MemoryCandidate {
                item_type: ContextItemType::Message,
                item_id: user_message.id.clone(),
                excerpt: user_message.content.clone(),
                sensitivity: Sensitivity::Private,
                state: MemoryState::Confirmed,
                crisis_related: false,
                relevant: true,
                policy_allowed: true,
                selection_reason: "current_user_turn".to_owned(),
                relevance_score: 1_000,
                recorded_at: now,
            }],
            now,
        )?;
        vault.insert_context_packet(Actor::system(), &packet)?;
        let packet = vault.mark_context_packet_dispatched(Actor::user(), &packet.id, now)?;
        let provenance = Source::model(
            session.method.clone(),
            profile.name.clone(),
            profile.model.clone(),
            packet.id,
        );
        let assistant = SessionMessage::assistant(
            session.id.clone(),
            "A cautious synthetic response.",
            Sensitivity::Private,
            provenance.clone(),
            now + chrono::Duration::milliseconds(1),
        );
        vault.append_session_message(Actor::user(), &assistant)?;
        let duplicate_assistant = SessionMessage::assistant(
            session.id.clone(),
            "A second response without a new user turn.",
            Sensitivity::Private,
            provenance,
            now + chrono::Duration::milliseconds(2),
        );
        assert!(
            vault
                .append_session_message(Actor::user(), &duplicate_assistant)
                .is_err()
        );
        assert_eq!(
            vault
                .list_session_messages(Actor::user(), &session.id, 10)?
                .len(),
            2
        );

        profile
            .trusted_sensitivities
            .insert(Sensitivity::HighlySensitive);
        profile.updated_at += chrono::Duration::milliseconds(1);
        vault.put_provider_profile(Actor::user(), &profile)?;
        let high_user = SessionMessage::user(
            session.id.clone(),
            "A synthetic high-sensitivity question",
            Sensitivity::HighlySensitive,
            now + chrono::Duration::milliseconds(3),
        );
        vault.append_session_message(Actor::user(), &high_user)?;
        let high_packet = ContextBuilder::new(1)?.build(
            Actor::user(),
            "Synthetic sensitivity propagation",
            session.method.clone(),
            profile.id.clone(),
            profile.model.clone(),
            high_user.content.clone(),
            Sensitivity::HighlySensitive,
            vec![MemoryCandidate {
                item_type: ContextItemType::Message,
                item_id: high_user.id.clone(),
                excerpt: high_user.content.clone(),
                sensitivity: Sensitivity::HighlySensitive,
                state: MemoryState::Confirmed,
                crisis_related: false,
                relevant: true,
                policy_allowed: true,
                selection_reason: "current_high_sensitivity_turn".to_owned(),
                relevance_score: 1_000,
                recorded_at: high_user.created_at,
            }],
            now + chrono::Duration::milliseconds(3),
        )?;
        vault.insert_context_packet(Actor::system(), &high_packet)?;
        vault.approve_context_packet(
            Actor::user(),
            &high_packet.id,
            now + chrono::Duration::milliseconds(4),
        )?;
        let high_packet = vault.mark_context_packet_dispatched(
            Actor::user(),
            &high_packet.id,
            now + chrono::Duration::milliseconds(5),
        )?;
        let high_provenance = Source::model(
            session.method.clone(),
            profile.name.clone(),
            profile.model.clone(),
            high_packet.id,
        );
        let downgraded = SessionMessage::assistant(
            session.id.clone(),
            "A response that must retain the input sensitivity.",
            Sensitivity::Private,
            high_provenance.clone(),
            now + chrono::Duration::milliseconds(6),
        );
        assert!(
            vault
                .append_session_message(Actor::user(), &downgraded)
                .is_err()
        );
        let high_assistant = SessionMessage::assistant(
            session.id.clone(),
            "A response retaining the input sensitivity.",
            Sensitivity::HighlySensitive,
            high_provenance,
            now + chrono::Duration::milliseconds(6),
        );
        vault.append_session_message(Actor::user(), &high_assistant)?;
        assert_eq!(
            vault
                .list_session_messages(Actor::user(), &session.id, 10)?
                .len(),
            4
        );
        vault.close_session(
            Actor::user(),
            &session.id,
            true,
            now + chrono::Duration::milliseconds(7),
        )?;
        let late = SessionMessage::user(
            session.id,
            "A late synthetic message",
            Sensitivity::Private,
            now + chrono::Duration::milliseconds(8),
        );
        assert!(vault.append_session_message(Actor::user(), &late).is_err());
        Ok(())
    }

    #[test]
    fn deleting_a_session_repairs_claims_that_cited_user_messages() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let now = Utc::now();
        let session = ReflectionSession::user_started(
            "free-reflection@1.0.0",
            None,
            Sensitivity::Private,
            now,
        );
        vault.create_session(Actor::user(), &session)?;
        let message = SessionMessage::user(
            session.id.clone(),
            "A synthetic directly reported observation.",
            Sensitivity::Private,
            now,
        );
        vault.append_session_message(Actor::user(), &message)?;
        let mut claim = Claim::proposal(
            ClaimKind::ExplicitFact,
            "The synthetic observation was directly reported.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            Source::user(),
            now,
        );
        claim.supporting_evidence.insert(message.id);
        vault.insert_claim(Actor::user(), &claim)?;
        vault.review_claim(
            Actor::user(),
            &claim.id,
            ReviewRequest {
                decision: ReviewDecision::Confirm,
                edited_statement: None,
                note: None,
            },
            now,
        )?;
        let impact = vault.delete_session(Actor::user(), &session.id, now)?;
        assert_eq!(impact.affected_claims, vec![claim.id.clone()]);
        let repaired = vault
            .get_claim(Actor::user(), &claim.id)?
            .ok_or(VaultError::NotFoundRecord)?;
        assert_eq!(repaired.status, ClaimStatus::Contradicted);
        assert!(repaired.supporting_evidence.is_empty());
        Ok(())
    }
}
