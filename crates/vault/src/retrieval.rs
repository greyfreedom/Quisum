use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use quisum_core::{
    Actor, ActorKind, Capability, Claim, ClaimKind, ContextItemType, EntityId, EntityKind, Event,
    MemoryCandidate, MemoryState, OpenQuestion, Sensitivity, SessionMessage, UserCorrection,
    Validate,
};
use rusqlite::{Connection, Transaction, params};
use sha2::{Digest, Sha256};

use crate::{SqlCipherVault, VaultError, store};

const EMBEDDING_MODEL: &str = "quisum-local-feature-v1";
const EMBEDDING_DIMENSIONS: usize = 384;
const MAXIMUM_SCAN_RECORDS: usize = 5_000;
const MAX_CONTEXT_EXCERPT_CHARS: usize = 8_000;

#[derive(Debug)]
struct SearchableMemory {
    item_type: ContextItemType,
    item_id: EntityId,
    excerpt: String,
    sensitivity: Sensitivity,
    state: MemoryState,
    crisis_related: bool,
    recorded_at: DateTime<Utc>,
}

impl SqlCipherVault {
    /// Run bounded hybrid FTS/local-feature retrieval, then apply actor privacy rules.
    pub fn search_memory(
        &self,
        actor: Actor,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryCandidate>, VaultError> {
        actor.require(Capability::SearchMemory)?;
        let query = query.trim();
        if query.is_empty() || query.chars().count() > 500 || limit == 0 || limit > 100 {
            return Err(quisum_core::DomainError::InvalidValue("search.request").into());
        }
        let connection = self.connection.lock();
        let lexical = lexical_matches(&connection, query)?;
        let query_embedding = embed(query);
        let mut memories = load_searchable_memories(&connection)?;
        merge_exact_lexical_memories(&connection, lexical.keys(), &mut memories)?;
        let mut candidates = Vec::new();
        for memory in memories {
            if actor.kind() == ActorKind::ModelHost
                && (memory.crisis_related
                    || matches!(
                        memory.sensitivity,
                        Sensitivity::HighlySensitive | Sensitivity::LocalOnly
                    ))
            {
                continue;
            }
            let lexical_score = lexical.get(&memory.item_id).copied().unwrap_or(0.0);
            let semantic_score = load_embedding(&connection, &memory.item_id)?
                .map(|embedding| cosine(&query_embedding, &embedding))
                .unwrap_or(0.0);
            let semantic_match = semantic_score >= 0.24;
            if lexical_score <= 0.0 && !semantic_match {
                continue;
            }
            let reason = match (lexical_score > 0.0, semantic_match) {
                (true, true) => "lexical_and_local_similarity",
                (true, false) => "lexical_match",
                (false, true) => "local_similarity",
                (false, false) => continue,
            };
            let score = ((lexical_score.min(1.0) * 600.0)
                + (semantic_score.clamp(0.0, 1.0) * 400.0))
                .round()
                .clamp(1.0, 1_000.0) as u16;
            candidates.push(MemoryCandidate {
                item_type: memory.item_type,
                item_id: memory.item_id,
                excerpt: memory.excerpt,
                sensitivity: memory.sensitivity,
                state: memory.state,
                crisis_related: memory.crisis_related,
                relevant: true,
                policy_allowed: true,
                selection_reason: reason.to_owned(),
                relevance_score: score,
                recorded_at: memory.recorded_at,
            });
        }
        candidates.sort_by(|left, right| {
            right
                .relevance_score
                .cmp(&left.relevance_score)
                .then_with(|| right.recorded_at.cmp(&left.recorded_at))
                .then_with(|| left.item_id.cmp(&right.item_id))
        });
        candidates.truncate(limit);
        Ok(candidates)
    }

    /// Rebuild all on-device feature embeddings from eligible stored projections.
    pub fn rebuild_local_search_index(&self, actor: Actor) -> Result<u64, VaultError> {
        actor.require(Capability::MaintainVault)?;
        if actor.kind() != ActorKind::System {
            return Err(quisum_core::DomainError::Forbidden(Capability::MaintainVault).into());
        }
        let mut connection = self.connection.lock();
        let memories = load_searchable_memories(&connection)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM embeddings WHERE model_id = ?1",
            [EMBEDDING_MODEL],
        )?;
        for memory in &memories {
            upsert_embedding(
                &transaction,
                &memory.item_id,
                memory.item_type,
                &memory.excerpt,
                memory.recorded_at,
            )?;
        }
        transaction.commit()?;
        u64::try_from(memories.len())
            .map_err(|_| quisum_core::DomainError::InvalidValue("search.index_count").into())
    }
}

pub(crate) fn upsert_embedding(
    transaction: &Transaction<'_>,
    item_id: &EntityId,
    item_type: ContextItemType,
    text: &str,
    updated_at: DateTime<Utc>,
) -> Result<(), VaultError> {
    let vector = encode_vector(&embed(text));
    transaction.execute(
        "INSERT INTO embeddings(item_id, item_type, model_id, dimensions, vector, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(item_id, model_id) DO UPDATE SET
           item_type = excluded.item_type,
           dimensions = excluded.dimensions,
           vector = excluded.vector,
           updated_at = excluded.updated_at",
        params![
            item_id.to_string(),
            store::enum_token(&item_type)?,
            EMBEDDING_MODEL,
            i64::try_from(EMBEDDING_DIMENSIONS).map_err(|_| {
                quisum_core::DomainError::InvalidValue("search.embedding_dimensions")
            })?,
            vector,
            store::format_time(updated_at)
        ],
    )?;
    Ok(())
}

pub(crate) fn remove_embedding(
    transaction: &Transaction<'_>,
    item_id: &EntityId,
) -> Result<(), VaultError> {
    transaction.execute(
        "DELETE FROM embeddings WHERE item_id = ?1 AND model_id = ?2",
        params![item_id.to_string(), EMBEDDING_MODEL],
    )?;
    Ok(())
}

fn lexical_matches(
    connection: &Connection,
    query: &str,
) -> Result<BTreeMap<EntityId, f32>, VaultError> {
    let Some(expression) = fts_expression(query) else {
        return Ok(BTreeMap::new());
    };
    let mut scores: BTreeMap<EntityId, f32> = BTreeMap::new();
    for (table, weight) in [
        ("events_fts", 1.0_f32),
        ("claims_fts", 1.0_f32),
        ("corrections_fts", 1.15_f32),
    ] {
        let sql = format!("SELECT id, bm25({table}) FROM {table} WHERE {table} MATCH ?1 LIMIT 256");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([&expression], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (identifier, rank) = row?;
            let identifier = identifier.parse::<EntityId>()?;
            let normalized = (1.0 / (1.0 + rank.abs() as f32)) * weight;
            scores
                .entry(identifier)
                .and_modify(|current| *current = current.max(normalized))
                .or_insert(normalized);
        }
    }
    Ok(scores)
}

fn load_searchable_memories(connection: &Connection) -> Result<Vec<SearchableMemory>, VaultError> {
    let mut memories = Vec::new();
    load_payloads(
        connection,
        "SELECT m.payload_json
         FROM messages m
         JOIN sessions s ON s.id = m.session_id
         WHERE m.role = 'user' AND m.crisis_related = 0 AND s.status != 'deleted'
         ORDER BY m.created_at DESC LIMIT ?1",
        |payload| {
            let message: SessionMessage = serde_json::from_str(payload)?;
            message.validate()?;
            Ok(SearchableMemory {
                item_type: ContextItemType::Message,
                item_id: message.id,
                excerpt: context_excerpt(&message.content),
                sensitivity: message.sensitivity,
                state: MemoryState::Confirmed,
                crisis_related: message.crisis_related,
                recorded_at: message.created_at,
            })
        },
        &mut memories,
    )?;
    load_payloads(
        connection,
        "SELECT payload_json FROM events WHERE status = 'confirmed' ORDER BY recorded_at DESC LIMIT ?1",
        |payload| {
            let event: Event = serde_json::from_str(payload)?;
            event.validate()?;
            Ok(SearchableMemory {
                item_type: ContextItemType::Event,
                item_id: event.id,
                excerpt: context_excerpt(&format!("{}\n{}", event.title, event.facts.join("\n"))),
                sensitivity: event.sensitivity,
                state: MemoryState::Confirmed,
                crisis_related: event.crisis_related,
                recorded_at: event.recorded_at,
            })
        },
        &mut memories,
    )?;
    load_payloads(
        connection,
        "SELECT payload_json FROM claims WHERE status IN ('confirmed', 'revised', 'contradicted') ORDER BY updated_at DESC LIMIT ?1",
        |payload| {
            let claim: Claim = serde_json::from_str(payload)?;
            claim.validate()?;
            let state = if claim.kind == ClaimKind::CurrentState
                && claim.revisit_after.is_some_and(|until| until <= Utc::now())
            {
                MemoryState::Expired
            } else {
                MemoryState::Confirmed
            };
            Ok(SearchableMemory {
                item_type: ContextItemType::Claim,
                item_id: claim.id,
                excerpt: context_excerpt(&claim.statement),
                sensitivity: claim.sensitivity,
                state,
                crisis_related: claim.crisis_related,
                recorded_at: claim.updated_at,
            })
        },
        &mut memories,
    )?;
    load_payloads(
        connection,
        "SELECT payload_json FROM user_corrections WHERE active = 1 ORDER BY updated_at DESC LIMIT ?1",
        |payload| {
            let correction: UserCorrection = serde_json::from_str(payload)?;
            correction.validate()?;
            Ok(SearchableMemory {
                item_type: ContextItemType::Correction,
                item_id: correction.id,
                excerpt: context_excerpt(&correction.statement),
                sensitivity: correction.sensitivity,
                state: MemoryState::Active,
                crisis_related: false,
                recorded_at: correction.updated_at,
            })
        },
        &mut memories,
    )?;
    load_payloads(
        connection,
        "SELECT payload_json FROM open_questions WHERE status = 'open' ORDER BY created_at DESC LIMIT ?1",
        |payload| {
            let question: OpenQuestion = serde_json::from_str(payload)?;
            question.validate()?;
            Ok(SearchableMemory {
                item_type: ContextItemType::OpenQuestion,
                item_id: question.id,
                excerpt: context_excerpt(&question.question),
                sensitivity: question.sensitivity,
                state: MemoryState::Active,
                crisis_related: question.crisis_related,
                recorded_at: question.created_at,
            })
        },
        &mut memories,
    )?;
    load_payloads(
        connection,
        "SELECT payload_json FROM experiments WHERE status = 'active' ORDER BY planned_at DESC LIMIT ?1",
        |payload| {
            let experiment: quisum_core::BehavioralExperiment = serde_json::from_str(payload)?;
            experiment.validate()?;
            Ok(SearchableMemory {
                item_type: ContextItemType::Experiment,
                item_id: experiment.id,
                excerpt: context_excerpt(&format!(
                    "{}\n{}",
                    experiment.title, experiment.hypothesis
                )),
                sensitivity: experiment.sensitivity,
                state: MemoryState::Active,
                crisis_related: experiment.crisis_related,
                recorded_at: experiment.planned_at,
            })
        },
        &mut memories,
    )?;
    Ok(memories)
}

fn merge_exact_lexical_memories<'a>(
    connection: &Connection,
    identifiers: impl Iterator<Item = &'a EntityId>,
    output: &mut Vec<SearchableMemory>,
) -> Result<(), VaultError> {
    use rusqlite::OptionalExtension as _;

    let mut seen = output
        .iter()
        .map(|memory| memory.item_id.clone())
        .collect::<BTreeSet<_>>();
    for identifier in identifiers {
        if seen.contains(identifier) {
            continue;
        }
        let query = match identifier.kind() {
            EntityKind::Event => {
                "SELECT payload_json FROM events WHERE id = ?1 AND status = 'confirmed'"
            }
            EntityKind::Claim => {
                "SELECT payload_json FROM claims
                 WHERE id = ?1 AND status IN ('confirmed', 'revised', 'contradicted')"
            }
            EntityKind::Correction => {
                "SELECT payload_json FROM user_corrections WHERE id = ?1 AND active = 1"
            }
            _ => continue,
        };
        let payload = connection
            .query_row(query, [identifier.to_string()], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        let Some(payload) = payload else {
            continue;
        };
        let memory = match identifier.kind() {
            EntityKind::Event => {
                let event: Event = serde_json::from_str(&payload)?;
                event.validate()?;
                SearchableMemory {
                    item_type: ContextItemType::Event,
                    item_id: event.id,
                    excerpt: context_excerpt(&format!(
                        "{}\n{}",
                        event.title,
                        event.facts.join("\n")
                    )),
                    sensitivity: event.sensitivity,
                    state: MemoryState::Confirmed,
                    crisis_related: event.crisis_related,
                    recorded_at: event.recorded_at,
                }
            }
            EntityKind::Claim => {
                let claim: Claim = serde_json::from_str(&payload)?;
                claim.validate()?;
                let state = if claim.kind == ClaimKind::CurrentState
                    && claim.revisit_after.is_some_and(|until| until <= Utc::now())
                {
                    MemoryState::Expired
                } else {
                    MemoryState::Confirmed
                };
                SearchableMemory {
                    item_type: ContextItemType::Claim,
                    item_id: claim.id,
                    excerpt: context_excerpt(&claim.statement),
                    sensitivity: claim.sensitivity,
                    state,
                    crisis_related: claim.crisis_related,
                    recorded_at: claim.updated_at,
                }
            }
            EntityKind::Correction => {
                let correction: UserCorrection = serde_json::from_str(&payload)?;
                correction.validate()?;
                SearchableMemory {
                    item_type: ContextItemType::Correction,
                    item_id: correction.id,
                    excerpt: context_excerpt(&correction.statement),
                    sensitivity: correction.sensitivity,
                    state: MemoryState::Active,
                    crisis_related: false,
                    recorded_at: correction.updated_at,
                }
            }
            _ => continue,
        };
        seen.insert(memory.item_id.clone());
        output.push(memory);
    }
    Ok(())
}

fn load_payloads(
    connection: &Connection,
    sql: &str,
    parse: impl Fn(&str) -> Result<SearchableMemory, VaultError>,
    output: &mut Vec<SearchableMemory>,
) -> Result<(), VaultError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        [i64::try_from(MAXIMUM_SCAN_RECORDS)
            .map_err(|_| quisum_core::DomainError::InvalidValue("search.scan_limit"))?],
        |row| row.get::<_, String>(0),
    )?;
    for row in rows {
        output.push(parse(&row?)?);
    }
    Ok(())
}

pub(crate) fn context_excerpt(value: &str) -> String {
    if value.chars().count() <= MAX_CONTEXT_EXCERPT_CHARS {
        return value.to_owned();
    }
    let mut excerpt = value
        .chars()
        .take(MAX_CONTEXT_EXCERPT_CHARS - 1)
        .collect::<String>();
    excerpt.push('…');
    excerpt
}

fn load_embedding(
    connection: &Connection,
    item_id: &EntityId,
) -> Result<Option<Vec<f32>>, VaultError> {
    use rusqlite::OptionalExtension as _;
    let bytes = connection
        .query_row(
            "SELECT vector FROM embeddings
             WHERE item_id = ?1 AND model_id = ?2 AND dimensions = ?3",
            params![
                item_id.to_string(),
                EMBEDDING_MODEL,
                i64::try_from(EMBEDDING_DIMENSIONS).map_err(|_| {
                    quisum_core::DomainError::InvalidValue("search.embedding_dimensions")
                })?
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    bytes.map(|bytes| decode_vector(&bytes)).transpose()
}

fn fts_expression(query: &str) -> Option<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in query.chars() {
        if character.is_alphanumeric() {
            current.extend(character.to_lowercase());
            if current.chars().count() == 64 {
                terms.push(std::mem::take(&mut current));
            }
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
        if terms.len() == 16 {
            break;
        }
    }
    if terms.len() < 16 && !current.is_empty() {
        terms.push(current);
    }
    terms.sort();
    terms.dedup();
    (!terms.is_empty()).then(|| {
        terms
            .into_iter()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn embed(text: &str) -> Vec<f32> {
    let normalized: Vec<char> = text
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric() || character.is_whitespace())
        .collect();
    let mut vector = vec![0.0_f32; EMBEDDING_DIMENSIONS];
    let mut add_feature = |kind: u8, value: &[u8], weight: f32| {
        let mut hasher = Sha256::new();
        hasher.update([kind]);
        hasher.update(value);
        let digest = hasher.finalize();
        let index = (usize::from(digest[0]) << 8 | usize::from(digest[1])) % EMBEDDING_DIMENSIONS;
        let sign = if digest[2] & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign * weight;
    };

    let normalized_text: String = normalized.iter().collect();
    for token in normalized_text
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .take(512)
    {
        add_feature(b'w', token.as_bytes(), 1.8);
    }
    for width in [2_usize, 3] {
        for window in normalized.windows(width).take(2_048) {
            let feature: String = window.iter().collect();
            add_feature(width as u8, feature.as_bytes(), 0.55);
        }
    }
    let magnitude = vector
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
        .sqrt();
    if magnitude > f32::EPSILON {
        for component in &mut vector {
            *component /= magnitude;
        }
    }
    vector
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != EMBEDDING_DIMENSIONS || right.len() != EMBEDDING_DIMENSIONS {
        return 0.0;
    }
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f32>()
        .clamp(-1.0, 1.0)
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for component in vector {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    bytes
}

fn decode_vector(bytes: &[u8]) -> Result<Vec<f32>, VaultError> {
    if bytes.len() != EMBEDDING_DIMENSIONS * size_of::<f32>() {
        return Err(VaultError::IntegrityCheckFailed);
    }
    let mut vector = Vec::with_capacity(EMBEDDING_DIMENSIONS);
    for chunk in bytes.chunks_exact(size_of::<f32>()) {
        let array: [u8; size_of::<f32>()] = chunk
            .try_into()
            .map_err(|_| VaultError::IntegrityCheckFailed)?;
        let component = f32::from_le_bytes(array);
        if !component.is_finite() {
            return Err(VaultError::IntegrityCheckFailed);
        }
        vector.push(component);
    }
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use tempfile::tempdir;

    use super::*;
    use crate::VaultKey;

    #[test]
    fn hybrid_search_finds_relevant_records_and_hides_local_only_from_model_hosts()
    -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let public = Event::user_confirmed(
            "Team disagreement",
            vec!["I delayed raising a concern during the planning meeting.".to_owned()],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        let secret = Event::user_confirmed(
            "Private journal marker",
            vec!["unshareable-synthetic-marker".to_owned()],
            None,
            Utc::now(),
            Sensitivity::LocalOnly,
        );
        vault.insert_event(Actor::user(), &public)?;
        vault.insert_event(Actor::user(), &secret)?;

        let results = vault.search_memory(Actor::user(), "planning concern", 10)?;
        assert!(
            results
                .iter()
                .any(|candidate| candidate.item_id == public.id)
        );
        assert!(
            vault
                .search_memory(Actor::model_host(), "unshareable synthetic marker", 10)?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn malformed_fts_syntax_is_always_quoted_as_data() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        assert!(
            vault
                .search_memory(Actor::user(), "\" OR * : -", 10)?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn search_truncates_large_records_to_the_context_packet_bound() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let event = Event::user_confirmed(
            "Large synthetic event",
            vec![
                format!("retrieval-marker {}", "x".repeat(3_980)),
                "y".repeat(4_000),
                "z".repeat(4_000),
            ],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;
        let result = vault
            .search_memory(Actor::user(), "retrieval marker", 1)?
            .into_iter()
            .next()
            .ok_or("large event was not retrieved")?;
        assert_eq!(result.excerpt.chars().count(), MAX_CONTEXT_EXCERPT_CHARS);
        assert!(result.excerpt.ends_with('…'));
        Ok(())
    }

    #[test]
    fn feature_vectors_round_trip_without_unsafe_casts() -> Result<(), Box<dyn Error>> {
        let vector = embed("synthetic repeated planning pattern");
        let decoded = decode_vector(&encode_vector(&vector))?;
        assert_eq!(vector, decoded);
        assert!(cosine(&vector, &decoded) > 0.999);
        Ok(())
    }

    #[test]
    fn exact_lexical_hits_merge_even_when_not_in_the_recent_semantic_scan()
    -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let event = Event::user_confirmed(
            "Archived lexical marker",
            vec!["old-exact-marker remains searchable".to_owned()],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;
        let connection = vault.connection.lock();
        let lexical = lexical_matches(&connection, "old exact marker")?;
        let mut memories = Vec::new();
        merge_exact_lexical_memories(&connection, lexical.keys(), &mut memories)?;
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].item_id, event.id);
        Ok(())
    }
}
