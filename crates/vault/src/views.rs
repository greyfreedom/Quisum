use chrono::{DateTime, Utc};
use quisum_core::{
    Actor, ActorKind, AuditEvent, BehavioralExperiment, Capability, Claim, ClaimRevision,
    ContextPacket, EntityId, EntityKind, Event, OpenQuestion, Sensitivity, Validate,
};
use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;

use crate::{SqlCipherVault, VaultError};

/// Sequence-numbered append-only history entry for Reflog surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// Monotonic local audit sequence.
    pub sequence: u64,
    /// Content-free typed audit record.
    pub event: AuditEvent,
}

impl SqlCipherVault {
    /// Read one event through the bounded-record capability.
    pub fn get_event(
        &self,
        actor: Actor,
        event_id: &EntityId,
    ) -> Result<Option<Event>, VaultError> {
        actor.require(Capability::ReadBoundedRecord)?;
        event_id.require_kind(EntityKind::Event)?;
        let connection = self.connection.lock();
        let event = load_optional::<Event>(
            &connection,
            "SELECT payload_json FROM events WHERE id = ?1",
            event_id,
        )?;
        if let Some(event) = &event {
            event.validate()?;
            if actor.kind() == ActorKind::ModelHost
                && (event.status != quisum_core::EventStatus::Confirmed
                    || event.crisis_related
                    || matches!(
                        event.sensitivity,
                        Sensitivity::HighlySensitive | Sensitivity::LocalOnly
                    ))
            {
                return Ok(None);
            }
        }
        Ok(event)
    }

    /// List events for the local timeline.
    pub fn list_events(&self, actor: Actor, limit: usize) -> Result<Vec<Event>, VaultError> {
        require_local_user(actor, Capability::ReadBoundedRecord)?;
        require_limit(limit, 500, "event.list_limit")?;
        load_many(
            &self.connection.lock(),
            "SELECT payload_json FROM events ORDER BY recorded_at DESC, id LIMIT ?1",
            limit,
        )
    }

    /// List claim projections for Self Map and CLI views.
    pub fn list_claims(&self, actor: Actor, limit: usize) -> Result<Vec<Claim>, VaultError> {
        require_local_user(actor, Capability::ReadBoundedRecord)?;
        require_limit(limit, 500, "claim.list_limit")?;
        load_many(
            &self.connection.lock(),
            "SELECT payload_json FROM claims ORDER BY updated_at DESC, id LIMIT ?1",
            limit,
        )
    }

    /// List immutable revisions for one claim or the complete Reflog view.
    pub fn list_claim_revisions(
        &self,
        actor: Actor,
        claim_id: Option<&EntityId>,
        limit: usize,
    ) -> Result<Vec<ClaimRevision>, VaultError> {
        require_local_user(actor, Capability::ReviewClaim)?;
        require_limit(limit, 500, "revision.list_limit")?;
        if let Some(claim_id) = claim_id {
            claim_id.require_kind(EntityKind::Claim)?;
        }
        let connection = self.connection.lock();
        let sql = if claim_id.is_some() {
            "SELECT id, claim_id, kind, actor, reason, before_json, after_json, created_at
             FROM claim_revisions WHERE claim_id = ?1 ORDER BY created_at DESC, id LIMIT ?2"
        } else {
            "SELECT id, claim_id, kind, actor, reason, before_json, after_json, created_at
             FROM claim_revisions ORDER BY created_at DESC, id LIMIT ?2"
        };
        let mut statement = connection.prepare(sql)?;
        let limit = i64::try_from(limit)
            .map_err(|_| quisum_core::DomainError::InvalidValue("revision.list_limit"))?;
        let mut revisions = Vec::new();
        if let Some(claim_id) = claim_id {
            let rows = statement.query_map(params![claim_id.to_string(), limit], parse_revision)?;
            for row in rows {
                let revision = row?;
                revision.validate()?;
                revisions.push(revision);
            }
        } else {
            let rows =
                statement.query_map(params![Option::<String>::None, limit], parse_revision)?;
            for row in rows {
                let revision = row?;
                revision.validate()?;
                revisions.push(revision);
            }
        }
        Ok(revisions)
    }

    /// List experiments for the user-facing experiment tracker.
    pub fn list_experiments(
        &self,
        actor: Actor,
        limit: usize,
    ) -> Result<Vec<BehavioralExperiment>, VaultError> {
        require_local_user(actor, Capability::ReadBoundedRecord)?;
        require_limit(limit, 500, "experiment.list_limit")?;
        load_many(
            &self.connection.lock(),
            "SELECT payload_json FROM experiments ORDER BY planned_at DESC, id LIMIT ?1",
            limit,
        )
    }

    /// Fetch one open question through bounded-record policy.
    pub fn get_open_question(
        &self,
        actor: Actor,
        question_id: &EntityId,
    ) -> Result<Option<OpenQuestion>, VaultError> {
        actor.require(Capability::ReadBoundedRecord)?;
        question_id.require_kind(EntityKind::OpenQuestion)?;
        let connection = self.connection.lock();
        let question = load_optional::<OpenQuestion>(
            &connection,
            "SELECT payload_json FROM open_questions WHERE id = ?1",
            question_id,
        )?;
        if let Some(question) = &question {
            question.validate()?;
            if actor.kind() == ActorKind::ModelHost
                && (question.status != quisum_core::OpenQuestionStatus::Open
                    || question.crisis_related
                    || matches!(
                        question.sensitivity,
                        Sensitivity::HighlySensitive | Sensitivity::LocalOnly
                    ))
            {
                return Ok(None);
            }
        }
        Ok(question)
    }

    /// List open-question projections for local Review and Explore surfaces.
    pub fn list_open_questions(
        &self,
        actor: Actor,
        limit: usize,
    ) -> Result<Vec<OpenQuestion>, VaultError> {
        require_local_user(actor, Capability::ReadBoundedRecord)?;
        require_limit(limit, 500, "open_question.list_limit")?;
        load_many(
            &self.connection.lock(),
            "SELECT payload_json FROM open_questions ORDER BY created_at DESC, id LIMIT ?1",
            limit,
        )
    }

    /// Fetch one packet. Model hosts can retrieve only the final dispatched payload.
    pub fn get_context_packet(
        &self,
        actor: Actor,
        context_id: &EntityId,
    ) -> Result<Option<ContextPacket>, VaultError> {
        actor.require(Capability::ReadBoundedRecord)?;
        context_id.require_kind(EntityKind::Context)?;
        let connection = self.connection.lock();
        let packet = load_optional::<ContextPacket>(
            &connection,
            "SELECT payload_json FROM context_packets WHERE id = ?1",
            context_id,
        )?;
        if let Some(packet) = &packet {
            packet.validate()?;
            if actor.kind() == ActorKind::ModelHost
                && (packet.dispatched_at.is_none() || packet.authorize_egress().is_err())
            {
                return Ok(None);
            }
        }
        Ok(packet)
    }

    /// List structural history without returning event, message, or claim content.
    pub fn list_reflog(&self, actor: Actor, limit: usize) -> Result<Vec<ReflogEntry>, VaultError> {
        require_local_user(actor, Capability::ReadBoundedRecord)?;
        require_limit(limit, 1_000, "audit.list_limit")?;
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT sequence, id, actor, action, entity_type, entity_id, details_json, created_at
             FROM audit_events ORDER BY sequence DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(
            [i64::try_from(limit)
                .map_err(|_| quisum_core::DomainError::InvalidValue("audit.list_limit"))?],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )?;
        let mut entries = Vec::new();
        for row in rows {
            let (sequence, id, actor, action, entity_type, entity_id, details, created_at) = row?;
            let event = AuditEvent {
                schema_version: "1.0.0".to_owned(),
                id: id.parse()?,
                actor: parse_enum(&actor)?,
                action: parse_enum(&action)?,
                entity_type: entity_type.map(|value| parse_enum(&value)).transpose()?,
                entity_id: entity_id.map(|value| value.parse()).transpose()?,
                details: serde_json::from_str(&details)?,
                created_at: parse_time(&created_at)?,
            };
            event.validate()?;
            entries.push(ReflogEntry {
                sequence: u64::try_from(sequence).map_err(|_| VaultError::IntegrityCheckFailed)?,
                event,
            });
        }
        Ok(entries)
    }
}

fn load_optional<T: DeserializeOwned>(
    connection: &rusqlite::Connection,
    sql: &str,
    id: &EntityId,
) -> Result<Option<T>, VaultError> {
    connection
        .query_row(sql, [id.to_string()], |row| row.get::<_, String>(0))
        .optional()?
        .map(|payload| serde_json::from_str(&payload).map_err(VaultError::from))
        .transpose()
}

fn load_many<T: DeserializeOwned + Validate>(
    connection: &rusqlite::Connection,
    sql: &str,
    limit: usize,
) -> Result<Vec<T>, VaultError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        [i64::try_from(limit)
            .map_err(|_| quisum_core::DomainError::InvalidValue("view.list_limit"))?],
        |row| row.get::<_, String>(0),
    )?;
    let mut values = Vec::new();
    for row in rows {
        let value: T = serde_json::from_str(&row?)?;
        value.validate()?;
        values.push(value);
    }
    Ok(values)
}

fn parse_revision(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaimRevision> {
    let id: String = row.get(0)?;
    let claim_id: String = row.get(1)?;
    let kind: String = row.get(2)?;
    let actor: String = row.get(3)?;
    let reason: String = row.get(4)?;
    let before: String = row.get(5)?;
    let after: String = row.get(6)?;
    let created_at: String = row.get(7)?;
    let parse = || -> Result<ClaimRevision, VaultError> {
        Ok(ClaimRevision {
            id: id.parse()?,
            claim_id: claim_id.parse()?,
            kind: parse_enum(&kind)?,
            before: serde_json::from_str(&before)?,
            after: serde_json::from_str(&after)?,
            actor: parse_enum(&actor)?,
            reason,
            created_at: parse_time(&created_at)?,
        })
    };
    parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn require_local_user(actor: Actor, capability: Capability) -> Result<(), VaultError> {
    actor.require(capability)?;
    if actor.kind() == ActorKind::User {
        Ok(())
    } else {
        Err(quisum_core::DomainError::Forbidden(capability).into())
    }
}

fn require_limit(limit: usize, maximum: usize, field: &'static str) -> Result<(), VaultError> {
    if limit == 0 || limit > maximum {
        Err(quisum_core::DomainError::InvalidValue(field).into())
    } else {
        Ok(())
    }
}

fn parse_enum<T: DeserializeOwned>(value: &str) -> Result<T, VaultError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(VaultError::from)
}

fn parse_time(value: &str) -> Result<DateTime<Utc>, VaultError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| VaultError::IntegrityCheckFailed)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use tempfile::tempdir;

    use super::*;
    use crate::VaultKey;

    #[test]
    fn model_bounded_reads_hide_high_sensitivity_and_pending_records() -> Result<(), Box<dyn Error>>
    {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let event = Event::user_confirmed(
            "High sensitivity",
            vec!["Synthetic private detail.".to_owned()],
            None,
            Utc::now(),
            Sensitivity::HighlySensitive,
        );
        vault.insert_event(Actor::user(), &event)?;
        assert!(vault.get_event(Actor::user(), &event.id)?.is_some());
        assert!(vault.get_event(Actor::model_host(), &event.id)?.is_none());
        Ok(())
    }

    #[test]
    fn reflog_is_content_free_and_sequence_ordered() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let entries = vault.list_reflog(Actor::user(), 10)?;
        assert!(!entries.is_empty());
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].sequence > pair[1].sequence)
        );
        let json =
            serde_json::to_string(&entries.iter().map(|entry| &entry.event).collect::<Vec<_>>())?;
        assert!(!json.contains("vault.db"));
        Ok(())
    }
}
