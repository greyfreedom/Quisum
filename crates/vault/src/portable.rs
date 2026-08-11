use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use quisum_core::{
    Actor, ActorKind, AuditAction, AuditDetails, AuditEvent, BehavioralExperiment, Capability,
    Claim, ClaimRevision, ClaimRevisionKind, ClaimSnapshot, ClaimStatus, ContextItemType,
    ContextPacket, EntityId, EntityKind, Event, EventStatus, ExperimentStatus, MessageRole,
    OpenQuestion, OpenQuestionStatus, ProviderProfile, ReflectionSession, ReviewDecision,
    SafetyEngine, SessionMessage, SessionStatus, SourceActor, UserCorrection, UserEndorsement,
    UserReview, Validate,
};
use rusqlite::{Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    SqlCipherVault, VaultError,
    backup::{ArtifactGuard, file_error, resolve_new_destination},
    store,
};

const MAX_ARTIFACT_BYTES: u64 = 128 * 1_024 * 1_024;
const MAX_RECORD_BYTES: usize = 1_024 * 1_024;
const MAX_RECORDS: usize = 100_000;

/// Lossless portable serialization encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// One JSON package containing manifest and records.
    Json,
    /// Manifest line followed by one independently parseable record per line.
    Jsonl,
}

impl ExportFormat {
    const fn token(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonl => "jsonl",
        }
    }
}

/// Public portable-artifact manifest matching `schemas/export-manifest.schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportManifest {
    /// Public schema version.
    pub schema_version: String,
    /// Export operation identifier.
    pub export_id: EntityId,
    /// Quisum build version.
    pub product_version: String,
    /// Portable encoding.
    pub format: ExportFormat,
    /// Portable exports are plaintext; encrypted recovery uses `.qsb` backups.
    pub encrypted: bool,
    /// Export time.
    pub created_at: DateTime<Utc>,
    /// SHA-256 of the canonical records portion.
    pub records_sha256: String,
    /// Counts by record type.
    pub record_counts: BTreeMap<String, u64>,
}

/// Metadata returned after a portable export has been written, verified, and audited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportReceipt {
    /// Export identifier.
    pub export_id: EntityId,
    /// Canonical destination path.
    pub path: PathBuf,
    /// Encoding.
    pub format: ExportFormat,
    /// Number of exported records.
    pub record_count: u64,
    /// Records checksum.
    pub records_sha256: String,
}

/// Metadata returned after a validated portable import commits atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReceipt {
    /// Local import operation identifier.
    pub import_id: EntityId,
    /// Source export identifier.
    pub source_export_id: EntityId,
    /// Imported records.
    pub record_count: u64,
}

/// Metadata for a human-readable, non-restorable Markdown projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownExportReceipt {
    /// Export identifier.
    pub export_id: EntityId,
    /// Canonical destination path.
    pub path: PathBuf,
    /// Reviewed claims rendered.
    pub claim_count: u64,
    /// Confirmed events rendered.
    pub event_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "record_type", content = "record", rename_all = "snake_case")]
enum PortableRecord {
    ProviderProfile(ProviderProfile),
    ContextPacket(ContextPacket),
    Session(ReflectionSession),
    Message(SessionMessage),
    Event(Event),
    Claim(Claim),
    EvidenceLink(PortableEvidenceLink),
    UserReview(UserReview),
    ClaimRevision(ClaimRevision),
    UserCorrection(UserCorrection),
    OpenQuestion(OpenQuestion),
    Experiment(BehavioralExperiment),
}

impl PortableRecord {
    const fn kind_token(&self) -> &'static str {
        match self {
            Self::ProviderProfile(_) => "provider_profile",
            Self::ContextPacket(_) => "context_packet",
            Self::Session(_) => "session",
            Self::Message(_) => "message",
            Self::Event(_) => "event",
            Self::Claim(_) => "claim",
            Self::EvidenceLink(_) => "evidence_link",
            Self::UserReview(_) => "user_review",
            Self::ClaimRevision(_) => "claim_revision",
            Self::UserCorrection(_) => "user_correction",
            Self::OpenQuestion(_) => "open_question",
            Self::Experiment(_) => "experiment",
        }
    }

    fn identifier(&self) -> Option<&EntityId> {
        match self {
            Self::ProviderProfile(record) => Some(&record.id),
            Self::ContextPacket(record) => Some(&record.id),
            Self::Session(record) => Some(&record.id),
            Self::Message(record) => Some(&record.id),
            Self::Event(record) => Some(&record.id),
            Self::Claim(record) => Some(&record.id),
            Self::EvidenceLink(_) => None,
            Self::UserReview(record) => Some(&record.id),
            Self::ClaimRevision(record) => Some(&record.id),
            Self::UserCorrection(record) => Some(&record.id),
            Self::OpenQuestion(record) => Some(&record.id),
            Self::Experiment(record) => Some(&record.id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceRelation {
    Supporting,
    Counter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceReviewStatus {
    Proposed,
    Confirmed,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableEvidenceLink {
    claim_id: EntityId,
    evidence_id: EntityId,
    relation: EvidenceRelation,
    review_status: EvidenceReviewStatus,
    source_actor: ActorKind,
    context_packet_id: Option<EntityId>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonPackage {
    manifest: ExportManifest,
    records: Vec<PortableRecord>,
}

impl SqlCipherVault {
    /// Export the lossless, non-secret domain subset as checked JSON or JSONL.
    pub fn export_portable(
        &self,
        actor: Actor,
        destination: impl AsRef<Path>,
        format: ExportFormat,
        created_at: DateTime<Utc>,
    ) -> Result<ExportReceipt, VaultError> {
        require_user_portability_actor(actor, Capability::ExportAll)?;
        let destination = resolve_new_destination(destination.as_ref())?;
        let mut connection = self.connection.lock();
        require_supported_tables_empty(&connection)?;
        require_export_size_bound(&connection)?;
        let records = collect_records(&connection)?;
        if records.len() > MAX_RECORDS {
            return Err(VaultError::InvalidArtifact);
        }
        validate_record_graph(&records)?;
        let counts = count_records(&records)?;
        let records_bytes = serialize_records(&records, format)?;
        if u64::try_from(records_bytes.len()).map_err(|_| VaultError::InvalidArtifact)?
            > MAX_ARTIFACT_BYTES
        {
            return Err(VaultError::InvalidArtifact);
        }
        let checksum = hex::encode(Sha256::digest(&records_bytes));
        let manifest = ExportManifest {
            schema_version: "1.0.0".to_owned(),
            export_id: EntityId::new(EntityKind::Export),
            product_version: env!("CARGO_PKG_VERSION").to_owned(),
            format,
            encrypted: false,
            created_at,
            records_sha256: checksum.clone(),
            record_counts: counts,
        };
        validate_manifest(&manifest)?;
        let artifact = match format {
            ExportFormat::Json => serde_json::to_vec(&JsonPackage {
                manifest: manifest.clone(),
                records,
            })?,
            ExportFormat::Jsonl => {
                let mut output = serde_json::to_vec(&manifest)?;
                output.push(b'\n');
                output.extend_from_slice(&records_bytes);
                output
            }
        };
        if u64::try_from(artifact.len()).map_err(|_| VaultError::InvalidArtifact)?
            > MAX_ARTIFACT_BYTES
        {
            return Err(VaultError::InvalidArtifact);
        }
        let mut guard = ArtifactGuard::create(&destination)?;
        guard
            .file_mut()?
            .write_all(&artifact)
            .map_err(|error| file_error(&destination, error))?;
        guard.sync()?;
        let (verified_manifest, verified_records) = read_portable(&destination)?;
        if verified_manifest != manifest
            || count_records(&verified_records)? != manifest.record_counts
        {
            return Err(VaultError::ArtifactTampered);
        }
        let record_count =
            u64::try_from(verified_records.len()).map_err(|_| VaultError::InvalidArtifact)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ExportCreated,
            Some(manifest.export_id.clone()),
            AuditDetails::Artifact {
                format: format.token().to_owned(),
                record_count,
                encrypted: false,
            },
            created_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        guard.commit();
        Ok(ExportReceipt {
            export_id: manifest.export_id,
            path: destination,
            format,
            record_count,
            records_sha256: checksum,
        })
    }

    /// Import a checked portable export into a newly created empty vault in one transaction.
    pub fn import_portable(
        &self,
        actor: Actor,
        source: impl AsRef<Path>,
        imported_at: DateTime<Utc>,
    ) -> Result<ImportReceipt, VaultError> {
        require_user_portability_actor(actor, Capability::ImportAll)?;
        let source = source
            .as_ref()
            .canonicalize()
            .map_err(|error| file_error(source.as_ref(), error))?;
        if !source.is_file() {
            return Err(VaultError::InvalidArtifact);
        }
        let (manifest, mut records) = read_portable(&source)?;
        validate_record_graph(&records)?;
        let counts = count_records(&records)?;
        if counts != manifest.record_counts {
            return Err(VaultError::InvalidArtifact);
        }
        let import_id = EntityId::new(EntityKind::Import);
        isolate_imported_provider_credentials(&mut records, &import_id, imported_at)?;
        validate_record_graph(&records)?;
        let mut connection = self.connection.lock();
        require_import_target_empty(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_records(&transaction, &records)?;
        let record_count = u64::try_from(records.len()).map_err(|_| VaultError::InvalidArtifact)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ImportCompleted,
            Some(import_id.clone()),
            AuditDetails::AffectedRecords {
                count: record_count,
            },
            imported_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(ImportReceipt {
            import_id,
            source_export_id: manifest.export_id,
            record_count,
        })
    }

    /// Export a clearly derived, non-restorable Markdown view of reviewed events and claims.
    pub fn export_markdown(
        &self,
        actor: Actor,
        destination: impl AsRef<Path>,
        created_at: DateTime<Utc>,
    ) -> Result<MarkdownExportReceipt, VaultError> {
        require_user_portability_actor(actor, Capability::ExportAll)?;
        let destination = resolve_new_destination(destination.as_ref())?;
        let mut connection = self.connection.lock();
        require_markdown_size_bound(&connection)?;
        let events = load_typed::<Event>(
            &connection,
            "SELECT payload_json FROM events WHERE status = 'confirmed' ORDER BY recorded_at, id",
        )?;
        let claims = load_typed::<Claim>(
            &connection,
            "SELECT payload_json FROM claims WHERE status IN ('confirmed', 'revised', 'contradicted') ORDER BY updated_at, id",
        )?;
        for event in &events {
            event.validate()?;
        }
        for claim in &claims {
            claim.validate()?;
        }
        let export_id = EntityId::new(EntityKind::Export);
        let mut markdown = String::from(
            "# Quisum — derived self-inquiry view\n\n> This Markdown file is a human-readable projection, not a backup. Claims remain contextual, revisable, and non-diagnostic.\n\n",
        );
        markdown.push_str("## Reviewed claims\n\n");
        if claims.is_empty() {
            markdown.push_str("_No reviewed claims._\n\n");
        }
        for claim in &claims {
            markdown.push_str(&format!(
                "### {}\n\n- Status: `{}`\n- Evidence grade: `{}`\n- Supporting evidence: {}\n- Counterevidence: {}\n\n",
                claim.id,
                enum_display(&claim.status)?,
                enum_display(&claim.evidence_grade)?,
                claim.supporting_evidence.len(),
                claim.counterevidence.len()
            ));
            push_quoted(&mut markdown, &claim.statement);
            markdown.push('\n');
        }
        markdown.push_str("## Confirmed events\n\n");
        if events.is_empty() {
            markdown.push_str("_No confirmed events._\n");
        }
        for event in &events {
            markdown.push_str(&format!("### {} — {}\n\n", event.id, event.title));
            for fact in &event.facts {
                push_quoted(&mut markdown, fact);
            }
            markdown.push('\n');
        }
        if u64::try_from(markdown.len()).map_err(|_| VaultError::InvalidArtifact)?
            > MAX_ARTIFACT_BYTES
        {
            return Err(VaultError::InvalidArtifact);
        }
        let mut guard = ArtifactGuard::create(&destination)?;
        guard
            .file_mut()?
            .write_all(markdown.as_bytes())
            .map_err(|error| file_error(&destination, error))?;
        guard.sync()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ExportCreated,
            Some(export_id.clone()),
            AuditDetails::Artifact {
                format: "markdown_derived".to_owned(),
                record_count: u64::try_from(events.len() + claims.len())
                    .map_err(|_| VaultError::InvalidArtifact)?,
                encrypted: false,
            },
            created_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        guard.commit();
        Ok(MarkdownExportReceipt {
            export_id,
            path: destination,
            claim_count: u64::try_from(claims.len()).map_err(|_| VaultError::InvalidArtifact)?,
            event_count: u64::try_from(events.len()).map_err(|_| VaultError::InvalidArtifact)?,
        })
    }
}

fn isolate_imported_provider_credentials(
    records: &mut [PortableRecord],
    import_id: &EntityId,
    imported_at: DateTime<Utc>,
) -> Result<(), VaultError> {
    import_id.require_kind(EntityKind::Import)?;
    for record in records {
        let PortableRecord::ProviderProfile(profile) = record else {
            continue;
        };
        profile.enabled = false;
        if profile.credential_ref.is_some() {
            profile.credential_ref =
                Some(format!("import.{import_id}.{}", profile.id).to_ascii_lowercase());
        }
        profile.updated_at = if imported_at > profile.updated_at {
            imported_at
        } else {
            profile
                .updated_at
                .checked_add_signed(chrono::Duration::milliseconds(1))
                .ok_or(VaultError::InvalidArtifact)?
        };
        profile.validate()?;
    }
    Ok(())
}

fn require_export_size_bound(connection: &rusqlite::Connection) -> Result<(), VaultError> {
    let (record_count, record_bytes): (i64, i64) = connection.query_row(
        "SELECT COALESCE(SUM(record_count), 0), COALESCE(SUM(record_bytes), 0)
         FROM (
           SELECT COUNT(*) AS record_count, COALESCE(SUM(LENGTH(payload_json)), 0) AS record_bytes FROM provider_profiles
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM context_packets
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM sessions
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM messages
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM events
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM claims
           UNION ALL SELECT COUNT(*), COALESCE(SUM(
             LENGTH(claim_id) + LENGTH(evidence_id) + LENGTH(relation) +
             LENGTH(review_status) + LENGTH(source_actor) +
             LENGTH(COALESCE(context_packet_id, '')) + LENGTH(created_at)
           ), 0) FROM evidence_links
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM user_reviews
           UNION ALL SELECT COUNT(*), COALESCE(SUM(
             LENGTH(id) + LENGTH(claim_id) + LENGTH(kind) + LENGTH(actor) +
             LENGTH(reason) + LENGTH(before_json) + LENGTH(after_json) + LENGTH(created_at)
           ), 0) FROM claim_revisions
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM user_corrections
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM open_questions
           UNION ALL SELECT COUNT(*), COALESCE(SUM(LENGTH(payload_json)), 0) FROM experiments
         )",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    require_estimated_artifact_bound(record_count, record_bytes, 1, 512)
}

fn require_markdown_size_bound(connection: &rusqlite::Connection) -> Result<(), VaultError> {
    let (record_count, record_bytes): (i64, i64) = connection.query_row(
        "SELECT
           (SELECT COUNT(*) FROM events WHERE status = 'confirmed') +
           (SELECT COUNT(*) FROM claims WHERE status IN ('confirmed', 'revised', 'contradicted')),
           COALESCE((SELECT SUM(LENGTH(payload_json)) FROM events WHERE status = 'confirmed'), 0) +
           COALESCE((SELECT SUM(LENGTH(payload_json)) FROM claims WHERE status IN ('confirmed', 'revised', 'contradicted')), 0)",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    require_estimated_artifact_bound(record_count, record_bytes, 2, 1_024)
}

fn require_estimated_artifact_bound(
    record_count: i64,
    record_bytes: i64,
    byte_multiplier: u64,
    overhead_per_record: u64,
) -> Result<(), VaultError> {
    let record_count = u64::try_from(record_count).map_err(|_| VaultError::InvalidArtifact)?;
    let record_bytes = u64::try_from(record_bytes).map_err(|_| VaultError::InvalidArtifact)?;
    let estimated = record_bytes
        .checked_mul(byte_multiplier)
        .and_then(|bytes| {
            record_count
                .checked_mul(overhead_per_record)
                .and_then(|overhead| bytes.checked_add(overhead))
        })
        .ok_or(VaultError::InvalidArtifact)?;
    if record_count > MAX_RECORDS as u64 || estimated > MAX_ARTIFACT_BYTES {
        Err(VaultError::InvalidArtifact)
    } else {
        Ok(())
    }
}

fn collect_records(connection: &rusqlite::Connection) -> Result<Vec<PortableRecord>, VaultError> {
    let mut records = Vec::new();
    records.extend(
        load_typed::<ProviderProfile>(
            connection,
            "SELECT payload_json FROM provider_profiles ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::ProviderProfile),
    );
    records.extend(
        load_typed::<ContextPacket>(
            connection,
            "SELECT payload_json FROM context_packets ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::ContextPacket),
    );
    records.extend(
        load_typed::<ReflectionSession>(
            connection,
            "SELECT payload_json FROM sessions ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::Session),
    );
    records.extend(
        load_typed::<SessionMessage>(connection, "SELECT payload_json FROM messages ORDER BY id")?
            .into_iter()
            .map(PortableRecord::Message),
    );
    records.extend(
        load_typed::<Event>(connection, "SELECT payload_json FROM events ORDER BY id")?
            .into_iter()
            .map(PortableRecord::Event),
    );
    records.extend(
        load_typed::<Claim>(connection, "SELECT payload_json FROM claims ORDER BY id")?
            .into_iter()
            .map(PortableRecord::Claim),
    );
    {
        let mut statement = connection.prepare(
            "SELECT claim_id, evidence_id, relation, review_status,
                    source_actor, context_packet_id, created_at
             FROM evidence_links ORDER BY claim_id, evidence_id, relation",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        for row in rows {
            let (claim, evidence, relation, review_status, actor, context, created_at) = row?;
            records.push(PortableRecord::EvidenceLink(PortableEvidenceLink {
                claim_id: claim.parse()?,
                evidence_id: evidence.parse()?,
                relation: parse_enum(&relation)?,
                review_status: parse_enum(&review_status)?,
                source_actor: parse_enum(&actor)?,
                context_packet_id: context.map(|value| value.parse()).transpose()?,
                created_at: parse_time(&created_at)?,
            }));
        }
    }
    records.extend(
        load_typed::<UserReview>(
            connection,
            "SELECT payload_json FROM user_reviews ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::UserReview),
    );
    {
        let mut statement = connection.prepare(
            "SELECT id, claim_id, kind, actor, reason, before_json, after_json, created_at
             FROM claim_revisions ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        for row in rows {
            let (id, claim_id, kind, actor, reason, before, after, created_at) = row?;
            records.push(PortableRecord::ClaimRevision(ClaimRevision {
                id: id.parse()?,
                claim_id: claim_id.parse()?,
                kind: parse_enum(&kind)?,
                before: serde_json::from_str(&before)?,
                after: serde_json::from_str(&after)?,
                actor: parse_enum(&actor)?,
                reason,
                created_at: parse_time(&created_at)?,
            }));
        }
    }
    records.extend(
        load_typed::<UserCorrection>(
            connection,
            "SELECT payload_json FROM user_corrections ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::UserCorrection),
    );
    records.extend(
        load_typed::<OpenQuestion>(
            connection,
            "SELECT payload_json FROM open_questions ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::OpenQuestion),
    );
    records.extend(
        load_typed::<BehavioralExperiment>(
            connection,
            "SELECT payload_json FROM experiments ORDER BY id",
        )?
        .into_iter()
        .map(PortableRecord::Experiment),
    );
    Ok(records)
}

fn load_typed<T: DeserializeOwned>(
    connection: &rusqlite::Connection,
    sql: &str,
) -> Result<Vec<T>, VaultError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut records = Vec::new();
    for row in rows {
        records.push(serde_json::from_str(&row?)?);
    }
    Ok(records)
}

fn serialize_records(
    records: &[PortableRecord],
    format: ExportFormat,
) -> Result<Vec<u8>, VaultError> {
    for record in records {
        if serde_json::to_vec(record)?.len() > MAX_RECORD_BYTES {
            return Err(VaultError::InvalidArtifact);
        }
    }
    match format {
        ExportFormat::Json => serde_json::to_vec(records).map_err(VaultError::from),
        ExportFormat::Jsonl => {
            let mut bytes = Vec::new();
            for record in records {
                let line = serde_json::to_vec(record)?;
                if line.len() > MAX_RECORD_BYTES {
                    return Err(VaultError::InvalidArtifact);
                }
                bytes.extend_from_slice(&line);
                bytes.push(b'\n');
            }
            Ok(bytes)
        }
    }
}

fn read_portable(path: &Path) -> Result<(ExportManifest, Vec<PortableRecord>), VaultError> {
    let metadata = fs::metadata(path).map_err(|error| file_error(path, error))?;
    if metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(VaultError::InvalidArtifact);
    }
    let bytes = fs::read(path).map_err(|error| file_error(path, error))?;
    if let Ok(package) = serde_json::from_slice::<JsonPackage>(&bytes) {
        if package.records.len() > MAX_RECORDS || package.manifest.format != ExportFormat::Json {
            return Err(VaultError::InvalidArtifact);
        }
        for record in &package.records {
            if serde_json::to_vec(record)?.len() > MAX_RECORD_BYTES {
                return Err(VaultError::InvalidArtifact);
            }
        }
        validate_manifest(&package.manifest)?;
        let records_bytes = serde_json::to_vec(&package.records)?;
        require_checksum(&package.manifest, &records_bytes)?;
        return Ok((package.manifest, package.records));
    }
    let newline = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(VaultError::InvalidArtifact)?;
    if newline == 0 || newline > MAX_RECORD_BYTES {
        return Err(VaultError::InvalidArtifact);
    }
    let manifest: ExportManifest = serde_json::from_slice(&bytes[..newline])?;
    if manifest.format != ExportFormat::Jsonl {
        return Err(VaultError::InvalidArtifact);
    }
    validate_manifest(&manifest)?;
    let records_bytes = &bytes[newline + 1..];
    require_checksum(&manifest, records_bytes)?;
    let mut records = Vec::new();
    for line in records_bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_RECORD_BYTES || records.len() == MAX_RECORDS {
            return Err(VaultError::InvalidArtifact);
        }
        records.push(serde_json::from_slice(line)?);
    }
    Ok((manifest, records))
}

fn validate_manifest(manifest: &ExportManifest) -> Result<(), VaultError> {
    if manifest.schema_version != "1.0.0"
        || manifest.export_id.kind() != EntityKind::Export
        || manifest.product_version.is_empty()
        || manifest.product_version.len() > 120
        || manifest.encrypted
        || manifest.records_sha256.len() != 64
        || !manifest
            .records_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || manifest.record_counts.len() > 32
        || manifest.record_counts.keys().any(|key| {
            key.is_empty()
                || key.len() > 64
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
    {
        return Err(VaultError::InvalidArtifact);
    }
    Ok(())
}

fn require_checksum(manifest: &ExportManifest, records: &[u8]) -> Result<(), VaultError> {
    let actual = hex::encode(Sha256::digest(records));
    if actual == manifest.records_sha256 {
        Ok(())
    } else {
        Err(VaultError::ArtifactTampered)
    }
}

fn count_records(records: &[PortableRecord]) -> Result<BTreeMap<String, u64>, VaultError> {
    let mut counts = BTreeMap::new();
    for record in records {
        let count = counts
            .entry(record.kind_token().to_owned())
            .or_insert(0_u64);
        *count = count.checked_add(1).ok_or(VaultError::InvalidArtifact)?;
    }
    Ok(counts)
}

fn validate_record_graph(records: &[PortableRecord]) -> Result<(), VaultError> {
    let mut identifiers = BTreeSet::new();
    let mut providers = BTreeSet::new();
    let mut provider_records = BTreeMap::new();
    let mut contexts = BTreeSet::new();
    let mut context_records = BTreeMap::new();
    let mut sessions = BTreeMap::new();
    let mut session_messages = BTreeMap::<EntityId, Vec<&SessionMessage>>::new();
    let mut messages = BTreeMap::new();
    let mut events = BTreeMap::new();
    let mut evidence = BTreeSet::new();
    let mut claims = BTreeMap::new();
    let mut corrections = BTreeMap::new();
    let mut questions = BTreeMap::new();
    let mut experiments = BTreeMap::new();
    let mut reviews = Vec::new();
    let mut revisions = BTreeMap::<EntityId, Vec<&ClaimRevision>>::new();
    let mut links = Vec::new();
    for record in records {
        if let Some(identifier) = record.identifier() {
            if !identifiers.insert(identifier.clone()) {
                return Err(VaultError::InvalidArtifact);
            }
        }
        match record {
            PortableRecord::ProviderProfile(profile) => {
                profile.validate()?;
                providers.insert(profile.id.clone());
                provider_records.insert(profile.id.clone(), profile);
            }
            PortableRecord::ContextPacket(packet) => {
                packet.validate()?;
                require_import_text_safety(SourceActor::User, &packet.query)?;
                contexts.insert(packet.id.clone());
                context_records.insert(packet.id.clone(), packet);
            }
            PortableRecord::Session(session) => {
                session.validate()?;
                if session.status == SessionStatus::Deleted || session.crisis_related {
                    return Err(VaultError::InvalidArtifact);
                }
                sessions.insert(session.id.clone(), session);
            }
            PortableRecord::Message(message) => {
                message.validate()?;
                require_import_text_safety(message.source.actor, &message.content)?;
                messages.insert(message.id.clone(), message);
                session_messages
                    .entry(message.session_id.clone())
                    .or_default()
                    .push(message);
                if message.role == MessageRole::User {
                    evidence.insert(message.id.clone());
                }
            }
            PortableRecord::Event(event) => {
                event.validate()?;
                require_import_record_safety(event.source.actor, event)?;
                if event.status == EventStatus::Deleted || event.crisis_related {
                    return Err(VaultError::InvalidArtifact);
                }
                events.insert(event.id.clone(), event);
                if event.status == EventStatus::Confirmed && !event.crisis_related {
                    evidence.insert(event.id.clone());
                }
            }
            PortableRecord::Claim(claim) => {
                claim.validate()?;
                require_import_record_safety(claim.source.actor, claim)?;
                if claim.status == ClaimStatus::Deleted || claim.crisis_related {
                    return Err(VaultError::InvalidArtifact);
                }
                claims.insert(claim.id.clone(), claim);
            }
            PortableRecord::EvidenceLink(link) => {
                validate_evidence_link(link)?;
                links.push(link);
            }
            PortableRecord::UserReview(review) => {
                review.validate()?;
                reviews.push(review);
            }
            PortableRecord::ClaimRevision(revision) => {
                revision.validate()?;
                revisions
                    .entry(revision.claim_id.clone())
                    .or_default()
                    .push(revision);
            }
            PortableRecord::UserCorrection(correction) => {
                correction.validate()?;
                corrections.insert(correction.id.clone(), correction);
            }
            PortableRecord::OpenQuestion(question) => {
                question.validate()?;
                require_import_record_safety(question.source.actor, question)?;
                if question.status == OpenQuestionStatus::Deleted || question.crisis_related {
                    return Err(VaultError::InvalidArtifact);
                }
                questions.insert(question.id.clone(), question);
            }
            PortableRecord::Experiment(experiment) => {
                experiment.validate()?;
                require_import_record_safety(experiment.source.actor, experiment)?;
                if experiment.status == ExperimentStatus::Deleted || experiment.crisis_related {
                    return Err(VaultError::InvalidArtifact);
                }
                experiments.insert(experiment.id.clone(), experiment);
            }
        }
    }
    for (session_id, messages) in &mut session_messages {
        let session = sessions
            .get(session_id)
            .ok_or(VaultError::InvalidArtifact)?;
        if session.status == SessionStatus::Deleted {
            return Err(VaultError::InvalidArtifact);
        }
        messages.sort_by_key(|message| (message.created_at, message.id.clone()));
        let mut preceding_user: Option<&SessionMessage> = None;
        let mut preceding_created_at = None;
        for message in messages {
            if message.created_at < session.started_at
                || session
                    .completed_at
                    .is_some_and(|completed_at| message.created_at > completed_at)
                || message.sensitivity < session.sensitivity
                || preceding_created_at.is_some_and(|created_at| message.created_at <= created_at)
            {
                return Err(VaultError::InvalidArtifact);
            }
            preceding_created_at = Some(message.created_at);
            if message.role == MessageRole::User {
                preceding_user = Some(message);
                continue;
            }
            let packet =
                require_model_source_context(&message.source, &context_records, &provider_records)?
                    .ok_or(VaultError::InvalidArtifact)?;
            let user_message = preceding_user.take().ok_or(VaultError::InvalidArtifact)?;
            let required_sensitivity = packet
                .items
                .iter()
                .map(|item| item.sensitivity)
                .fold(packet.query_sensitivity, quisum_core::Sensitivity::max);
            if user_message.content != packet.query
                || packet.method != session.method
                || message.sensitivity < required_sensitivity
                || packet
                    .dispatched_at
                    .is_none_or(|dispatched_at| dispatched_at > message.created_at)
            {
                return Err(VaultError::InvalidArtifact);
            }
        }
    }
    for record in records {
        match record {
            PortableRecord::ContextPacket(packet) => {
                let profile = provider_records
                    .get(&packet.provider_profile_id)
                    .ok_or(VaultError::InvalidArtifact)?;
                if !providers.contains(&packet.provider_profile_id)
                    || profile.model != packet.model
                    || packet
                        .exclusions
                        .iter()
                        .any(|exclusion| !identifiers.contains(&exclusion.item_id))
                {
                    return Err(VaultError::InvalidArtifact);
                }
                for item in &packet.items {
                    let matches_projection = match item.item_type {
                        ContextItemType::Message => {
                            messages.get(&item.item_id).is_some_and(|message| {
                                message.role == MessageRole::User
                                    && message.sensitivity == item.sensitivity
                                    && sessions.get(&message.session_id).is_some_and(|session| {
                                        session.status != SessionStatus::Deleted
                                    })
                                    && item.excerpt
                                        == crate::retrieval::context_excerpt(&message.content)
                            })
                        }
                        ContextItemType::SessionSummary => false,
                        ContextItemType::Event => events.get(&item.item_id).is_some_and(|event| {
                            event.status == EventStatus::Confirmed
                                && !event.crisis_related
                                && event.sensitivity == item.sensitivity
                                && item.excerpt
                                    == crate::retrieval::context_excerpt(&format!(
                                        "{}\n{}",
                                        event.title,
                                        event.facts.join("\n")
                                    ))
                        }),
                        ContextItemType::Claim => claims.get(&item.item_id).is_some_and(|claim| {
                            matches!(
                                claim.status,
                                ClaimStatus::Confirmed
                                    | ClaimStatus::Revised
                                    | ClaimStatus::Contradicted
                            ) && !claim.crisis_related
                                && claim.sensitivity == item.sensitivity
                                && !(claim.kind == quisum_core::ClaimKind::CurrentState
                                    && claim
                                        .revisit_after
                                        .is_some_and(|revisit| revisit <= packet.created_at))
                                && item.excerpt
                                    == crate::retrieval::context_excerpt(&claim.statement)
                        }),
                        ContextItemType::Correction => {
                            corrections.get(&item.item_id).is_some_and(|correction| {
                                correction.active
                                    && correction.sensitivity == item.sensitivity
                                    && item.excerpt
                                        == crate::retrieval::context_excerpt(&correction.statement)
                            })
                        }
                        ContextItemType::OpenQuestion => {
                            questions.get(&item.item_id).is_some_and(|question| {
                                question.status == OpenQuestionStatus::Open
                                    && !question.crisis_related
                                    && question.sensitivity == item.sensitivity
                                    && item.excerpt
                                        == crate::retrieval::context_excerpt(&question.question)
                            })
                        }
                        ContextItemType::Experiment => {
                            experiments.get(&item.item_id).is_some_and(|experiment| {
                                experiment.status == ExperimentStatus::Active
                                    && !experiment.crisis_related
                                    && experiment.sensitivity == item.sensitivity
                                    && item.excerpt
                                        == crate::retrieval::context_excerpt(&format!(
                                            "{}\n{}",
                                            experiment.title, experiment.hypothesis
                                        ))
                            })
                        }
                    };
                    if !matches_projection {
                        return Err(VaultError::InvalidArtifact);
                    }
                }
            }
            PortableRecord::Session(_) => {}
            PortableRecord::Message(_) => {}
            PortableRecord::Event(event) => {
                require_model_source_context(&event.source, &context_records, &provider_records)?;
            }
            PortableRecord::Claim(claim) => {
                let model_packet = require_model_source_context(
                    &claim.source,
                    &context_records,
                    &provider_records,
                )?;
                if claim
                    .supporting_evidence
                    .iter()
                    .chain(claim.counterevidence.iter())
                    .any(|item| !evidence.contains(item))
                {
                    return Err(VaultError::InvalidArtifact);
                }
                if model_packet.is_some_and(|packet| {
                    claim
                        .supporting_evidence
                        .iter()
                        .chain(claim.counterevidence.iter())
                        .any(|evidence_id| {
                            !packet.items.iter().any(|item| item.item_id == *evidence_id)
                        })
                }) {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            PortableRecord::EvidenceLink(link) => {
                if !claims.contains_key(&link.claim_id)
                    || !evidence.contains(&link.evidence_id)
                    || link
                        .context_packet_id
                        .as_ref()
                        .is_some_and(|context| !contexts.contains(context))
                {
                    return Err(VaultError::InvalidArtifact);
                }
                if link.source_actor == ActorKind::ModelHost {
                    let packet = link
                        .context_packet_id
                        .as_ref()
                        .and_then(|id| context_records.get(id))
                        .ok_or(VaultError::InvalidArtifact)?;
                    packet.authorize_egress()?;
                    if packet.dispatched_at.is_none()
                        || [link.claim_id.clone(), link.evidence_id.clone()]
                            .iter()
                            .any(|required| {
                                !packet.items.iter().any(|item| item.item_id == *required)
                            })
                    {
                        return Err(VaultError::InvalidArtifact);
                    }
                }
            }
            PortableRecord::UserReview(review) => {
                if !claims.contains_key(&review.claim_id) {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            PortableRecord::ClaimRevision(revision) => {
                if !claims.contains_key(&revision.claim_id) {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            PortableRecord::UserCorrection(correction) => {
                if correction
                    .related_claims
                    .iter()
                    .any(|claim| !claims.contains_key(claim))
                {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            PortableRecord::OpenQuestion(question) => {
                if question
                    .related_claims
                    .iter()
                    .any(|claim| !claims.contains_key(claim))
                {
                    return Err(VaultError::InvalidArtifact);
                }
                let model_packet = require_model_source_context(
                    &question.source,
                    &context_records,
                    &provider_records,
                )?;
                if model_packet.is_some_and(|packet| {
                    question
                        .related_claims
                        .iter()
                        .any(|claim_id| !packet.items.iter().any(|item| item.item_id == *claim_id))
                }) {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            PortableRecord::Experiment(experiment) => {
                if experiment
                    .related_claims
                    .iter()
                    .any(|claim| !claims.contains_key(claim))
                {
                    return Err(VaultError::InvalidArtifact);
                }
                let model_packet = require_model_source_context(
                    &experiment.source,
                    &context_records,
                    &provider_records,
                )?;
                if model_packet.is_some_and(|packet| {
                    experiment
                        .related_claims
                        .iter()
                        .any(|claim_id| !packet.items.iter().any(|item| item.item_id == *claim_id))
                }) {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            PortableRecord::ProviderProfile(_) => {}
        }
    }
    validate_claim_histories(&claims, &reviews, &revisions)?;
    for (claim_id, claim) in claims {
        for evidence_id in &claim.supporting_evidence {
            if !links.iter().any(|link| {
                link.claim_id == claim_id
                    && link.evidence_id == *evidence_id
                    && link.relation == EvidenceRelation::Supporting
                    && link.review_status == EvidenceReviewStatus::Confirmed
            }) {
                return Err(VaultError::InvalidArtifact);
            }
        }
        for evidence_id in &claim.counterevidence {
            if !links.iter().any(|link| {
                link.claim_id == claim_id
                    && link.evidence_id == *evidence_id
                    && link.relation == EvidenceRelation::Counter
                    && link.review_status == EvidenceReviewStatus::Confirmed
            }) {
                return Err(VaultError::InvalidArtifact);
            }
        }
    }
    Ok(())
}

fn require_import_record_safety(
    actor: SourceActor,
    value: &impl Serialize,
) -> Result<(), VaultError> {
    require_import_text_safety(actor, &serde_json::to_string(value)?)
}

fn require_import_text_safety(actor: SourceActor, text: &str) -> Result<(), VaultError> {
    let decision = if actor == SourceActor::Model {
        SafetyEngine.review_generated_text(text)
    } else {
        SafetyEngine.screen_user_input(text)
    };
    SafetyEngine.require_persistence_allowed(&decision)?;
    Ok(())
}

fn validate_claim_histories(
    claims: &BTreeMap<EntityId, &Claim>,
    reviews: &[&UserReview],
    revisions: &BTreeMap<EntityId, Vec<&ClaimRevision>>,
) -> Result<(), VaultError> {
    if reviews
        .iter()
        .any(|review| !claims.contains_key(&review.claim_id))
        || revisions
            .keys()
            .any(|claim_id| !claims.contains_key(claim_id))
    {
        return Err(VaultError::InvalidArtifact);
    }

    for (claim_id, claim) in claims {
        let claim_revisions = revisions.get(claim_id).map(Vec::as_slice).unwrap_or(&[]);
        let claim_reviews = reviews
            .iter()
            .copied()
            .filter(|review| review.claim_id == *claim_id)
            .collect::<Vec<_>>();
        if claim_revisions.is_empty() {
            if !claim_reviews.is_empty()
                || (claim.source.actor == SourceActor::Model
                    && !matches!(
                        claim.status,
                        ClaimStatus::Proposed | ClaimStatus::PendingReview
                    ))
            {
                return Err(VaultError::InvalidArtifact);
            }
            continue;
        }

        let mut by_before = BTreeMap::new();
        let mut after_keys = BTreeSet::new();
        for (index, revision) in claim_revisions.iter().enumerate() {
            if revision.claim_id != *claim_id
                || revision.created_at < claim.created_at
                || revision.created_at > claim.updated_at
            {
                return Err(VaultError::InvalidArtifact);
            }
            let before = snapshot_digest(&revision.before)?;
            if by_before.insert(before, index).is_some() {
                return Err(VaultError::InvalidArtifact);
            }
            after_keys.insert(snapshot_digest(&revision.after)?);
        }
        let starts = by_before
            .iter()
            .filter_map(|(before, index)| (!after_keys.contains(before)).then_some(*index))
            .collect::<Vec<_>>();
        if starts.len() != 1 {
            return Err(VaultError::InvalidArtifact);
        }
        let mut visited = BTreeSet::new();
        let mut current = starts[0];
        let mut previous: Option<usize> = None;
        loop {
            if !visited.insert(current) {
                return Err(VaultError::InvalidArtifact);
            }
            let revision = claim_revisions[current];
            if let Some(previous_index) = previous {
                let preceding = claim_revisions[previous_index];
                if preceding.after != revision.before || revision.created_at < preceding.created_at
                {
                    return Err(VaultError::InvalidArtifact);
                }
            }
            let next = by_before.get(&snapshot_digest(&revision.after)?).copied();
            previous = Some(current);
            let Some(next) = next else {
                break;
            };
            current = next;
        }
        if visited.len() != claim_revisions.len()
            || claim_revisions[current].after != ClaimSnapshot::from(*claim)
        {
            return Err(VaultError::InvalidArtifact);
        }

        let human_revisions = claim_revisions
            .iter()
            .copied()
            .filter(|revision| revision.kind == ClaimRevisionKind::HumanReview)
            .collect::<Vec<_>>();
        let mut matched = BTreeSet::new();
        for review in claim_reviews {
            if review.reviewed_at < claim.created_at || review.reviewed_at > claim.updated_at {
                return Err(VaultError::InvalidArtifact);
            }
            let matching = human_revisions
                .iter()
                .copied()
                .filter(|revision| {
                    !matched.contains(&revision.id) && review_matches_revision(review, revision)
                })
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(VaultError::InvalidArtifact);
            }
            matched.insert(matching[0].id.clone());
        }
        if matched.len() != human_revisions.len()
            || (claim.source.actor == SourceActor::Model
                && !matches!(
                    claim.status,
                    ClaimStatus::Proposed | ClaimStatus::PendingReview
                )
                && human_revisions.is_empty())
        {
            return Err(VaultError::InvalidArtifact);
        }
    }
    Ok(())
}

fn snapshot_digest(snapshot: &ClaimSnapshot) -> Result<[u8; 32], VaultError> {
    Ok(Sha256::digest(serde_json::to_vec(snapshot)?).into())
}

fn review_matches_revision(review: &UserReview, revision: &ClaimRevision) -> bool {
    if revision.kind != ClaimRevisionKind::HumanReview
        || revision.actor != ActorKind::User
        || store::format_time(revision.created_at) != store::format_time(review.reviewed_at)
        || !matches!(
            revision.before.status,
            ClaimStatus::PendingReview | ClaimStatus::Uncertain | ClaimStatus::Contradicted
        )
        || revision.before.evidence_grade != revision.after.evidence_grade
        || revision.before.supporting_evidence != revision.after.supporting_evidence
        || revision.before.counterevidence != revision.after.counterevidence
        || revision.before.contexts != revision.after.contexts
    {
        return false;
    }
    match review.decision {
        ReviewDecision::Confirm => {
            revision.after.status == ClaimStatus::Confirmed
                && revision.after.user_endorsement == Some(UserEndorsement::Accepted)
                && revision.after.statement == revision.before.statement
        }
        ReviewDecision::ReviseAndConfirm => {
            revision.after.status == ClaimStatus::Revised
                && revision.after.user_endorsement == Some(UserEndorsement::Edited)
                && review
                    .edited_statement
                    .as_deref()
                    .is_some_and(|statement| revision.after.statement == statement.trim())
        }
        ReviewDecision::Uncertain => {
            revision.after.status == ClaimStatus::Uncertain
                && revision.after.user_endorsement == Some(UserEndorsement::Uncertain)
                && revision.after.statement == revision.before.statement
        }
        ReviewDecision::Reject | ReviewDecision::ConvertToOpenQuestion => {
            revision.after.status == ClaimStatus::Rejected
                && revision.after.user_endorsement == Some(UserEndorsement::Rejected)
                && revision.after.statement == revision.before.statement
        }
    }
}

fn require_model_source_context<'a>(
    source: &quisum_core::Source,
    contexts: &'a BTreeMap<EntityId, &ContextPacket>,
    providers: &BTreeMap<EntityId, &ProviderProfile>,
) -> Result<Option<&'a ContextPacket>, VaultError> {
    if source.actor != SourceActor::Model {
        return Ok(None);
    }
    let packet = source
        .context_packet_id
        .as_ref()
        .and_then(|context| contexts.get(context).copied())
        .ok_or(VaultError::InvalidArtifact)?;
    let profile = providers
        .get(&packet.provider_profile_id)
        .ok_or(VaultError::InvalidArtifact)?;
    packet.authorize_egress()?;
    if packet.dispatched_at.is_none()
        || profile.model != packet.model
        || source.method.as_deref() != Some(packet.method.as_str())
        || source.model.as_deref() != Some(packet.model.as_str())
        || source.provider.as_deref() != Some(profile.name.as_str())
    {
        return Err(VaultError::InvalidArtifact);
    }
    Ok(Some(packet))
}

fn validate_evidence_link(link: &PortableEvidenceLink) -> Result<(), VaultError> {
    link.claim_id.require_kind(EntityKind::Claim)?;
    if !matches!(
        link.evidence_id.kind(),
        EntityKind::Event
            | EntityKind::Message
            | EntityKind::SessionSummary
            | EntityKind::Assessment
    ) {
        return Err(VaultError::InvalidArtifact);
    }
    if let Some(context) = &link.context_packet_id {
        context.require_kind(EntityKind::Context)?;
    }
    if link.source_actor == ActorKind::ModelHost && link.context_packet_id.is_none() {
        return Err(VaultError::InvalidArtifact);
    }
    Ok(())
}

fn insert_records(
    transaction: &Transaction<'_>,
    records: &[PortableRecord],
) -> Result<(), VaultError> {
    for record in records {
        if let PortableRecord::ProviderProfile(profile) = record {
            transaction.execute(
                "INSERT INTO provider_profiles(
                   id, kind, privacy_kind, enabled, created_at, updated_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    profile.id.to_string(),
                    store::enum_token(&profile.kind)?,
                    store::enum_token(&profile.privacy_kind)?,
                    i64::from(profile.enabled),
                    store::format_time(profile.created_at),
                    store::format_time(profile.updated_at),
                    serde_json::to_string(profile)?
                ],
            )?;
        }
    }
    for record in records {
        if let PortableRecord::ContextPacket(packet) = record {
            transaction.execute(
                "INSERT INTO context_packets(
                   id, provider_profile_id, purpose, requires_approval,
                   approved_at, created_at, dispatched_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    packet.id.to_string(),
                    packet.provider_profile_id.to_string(),
                    packet.purpose,
                    i64::from(packet.requires_approval),
                    packet.approved_at.map(store::format_time),
                    store::format_time(packet.created_at),
                    packet.dispatched_at.map(store::format_time),
                    serde_json::to_string(packet)?
                ],
            )?;
        }
    }
    for record in records {
        if let PortableRecord::Session(session) = record {
            insert_session_raw(transaction, session)?;
        }
    }
    for record in records {
        if let PortableRecord::Message(message) = record {
            insert_message_raw(transaction, message)?;
        }
    }
    for record in records {
        match record {
            PortableRecord::Event(event) => insert_event_raw(transaction, event)?,
            PortableRecord::Claim(claim) => store::insert_claim_projection(transaction, claim)?,
            PortableRecord::UserCorrection(correction) => {
                insert_correction_raw(transaction, correction)?
            }
            PortableRecord::OpenQuestion(question) => {
                store::insert_open_question(transaction, question)?
            }
            PortableRecord::Experiment(experiment) => {
                insert_experiment_raw(transaction, experiment)?
            }
            PortableRecord::ProviderProfile(_)
            | PortableRecord::ContextPacket(_)
            | PortableRecord::Session(_)
            | PortableRecord::Message(_)
            | PortableRecord::EvidenceLink(_)
            | PortableRecord::UserReview(_)
            | PortableRecord::ClaimRevision(_) => {}
        }
    }
    for record in records {
        if let PortableRecord::EvidenceLink(link) = record {
            transaction.execute(
                "INSERT INTO evidence_links(
                   claim_id, evidence_id, relation, review_status,
                   source_actor, context_packet_id, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    link.claim_id.to_string(),
                    link.evidence_id.to_string(),
                    store::enum_token(&link.relation)?,
                    store::enum_token(&link.review_status)?,
                    store::enum_token(&link.source_actor)?,
                    link.context_packet_id.as_ref().map(ToString::to_string),
                    store::format_time(link.created_at)
                ],
            )?;
        }
    }
    for record in records {
        match record {
            PortableRecord::UserReview(review) => store::insert_review(transaction, review)?,
            PortableRecord::ClaimRevision(revision) => {
                store::insert_revision(transaction, revision)?
            }
            PortableRecord::ProviderProfile(_)
            | PortableRecord::ContextPacket(_)
            | PortableRecord::Session(_)
            | PortableRecord::Message(_)
            | PortableRecord::Event(_)
            | PortableRecord::Claim(_)
            | PortableRecord::EvidenceLink(_)
            | PortableRecord::UserCorrection(_)
            | PortableRecord::OpenQuestion(_)
            | PortableRecord::Experiment(_) => {}
        }
    }
    Ok(())
}

fn insert_session_raw(
    transaction: &Transaction<'_>,
    session: &ReflectionSession,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO sessions(
           id, method, status, started_at, completed_at,
           sensitivity, crisis_related, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            session.id.to_string(),
            session.method,
            store::enum_token(&session.status)?,
            store::format_time(session.started_at),
            session.completed_at.map(store::format_time),
            store::enum_token(&session.sensitivity)?,
            i64::from(session.crisis_related),
            serde_json::to_string(session)?
        ],
    )?;
    Ok(())
}

fn insert_message_raw(
    transaction: &Transaction<'_>,
    message: &SessionMessage,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO messages(
           id, session_id, role, created_at, sensitivity,
           crisis_related, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            message.id.to_string(),
            message.session_id.to_string(),
            store::enum_token(&message.role)?,
            store::format_time(message.created_at),
            store::enum_token(&message.sensitivity)?,
            i64::from(message.crisis_related),
            serde_json::to_string(message)?
        ],
    )?;
    if message.role == MessageRole::User {
        crate::retrieval::upsert_embedding(
            transaction,
            &message.id,
            ContextItemType::Message,
            &message.content,
            message.created_at,
        )?;
    }
    Ok(())
}

fn insert_event_raw(transaction: &Transaction<'_>, event: &Event) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO events(
           id, status, title, occurred_at, recorded_at,
           sensitivity, crisis_related, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            event.id.to_string(),
            store::enum_token(&event.status)?,
            event.title,
            event.occurred_at.map(store::format_time),
            store::format_time(event.recorded_at),
            store::enum_token(&event.sensitivity)?,
            i64::from(event.crisis_related),
            serde_json::to_string(event)?
        ],
    )?;
    if event.status == quisum_core::EventStatus::Confirmed {
        store::insert_event_search_index(transaction, event)?;
    }
    Ok(())
}

fn insert_correction_raw(
    transaction: &Transaction<'_>,
    correction: &UserCorrection,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO user_corrections(
           id, active, sensitivity, created_at, updated_at, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            correction.id.to_string(),
            i64::from(correction.active),
            store::enum_token(&correction.sensitivity)?,
            store::format_time(correction.created_at),
            store::format_time(correction.updated_at),
            serde_json::to_string(correction)?
        ],
    )?;
    if correction.active {
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
        crate::retrieval::upsert_embedding(
            transaction,
            &correction.id,
            ContextItemType::Correction,
            &correction.statement,
            correction.updated_at,
        )?;
    }
    Ok(())
}

fn insert_experiment_raw(
    transaction: &Transaction<'_>,
    experiment: &BehavioralExperiment,
) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO experiments(
           id, status, risk_level, planned_at, completed_at,
           sensitivity, crisis_related, payload_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            experiment.id.to_string(),
            store::enum_token(&experiment.status)?,
            store::enum_token(&experiment.risk_level)?,
            store::format_time(experiment.planned_at),
            experiment.completed_at.map(store::format_time),
            store::enum_token(&experiment.sensitivity)?,
            i64::from(experiment.crisis_related),
            serde_json::to_string(experiment)?
        ],
    )?;
    if experiment.status == ExperimentStatus::Active {
        crate::retrieval::upsert_embedding(
            transaction,
            &experiment.id,
            ContextItemType::Experiment,
            &format!("{}\n{}", experiment.title, experiment.hypothesis),
            experiment.planned_at,
        )?;
    }
    Ok(())
}

fn require_import_target_empty(connection: &rusqlite::Connection) -> Result<(), VaultError> {
    let occupied: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_profiles)
             OR EXISTS(SELECT 1 FROM context_packets)
             OR EXISTS(SELECT 1 FROM sessions)
             OR EXISTS(SELECT 1 FROM messages)
             OR EXISTS(SELECT 1 FROM events)
             OR EXISTS(SELECT 1 FROM claims)
             OR EXISTS(SELECT 1 FROM user_corrections)
             OR EXISTS(SELECT 1 FROM open_questions)
             OR EXISTS(SELECT 1 FROM experiments)",
        [],
        |row| row.get(0),
    )?;
    if occupied {
        Err(VaultError::Conflict)
    } else {
        Ok(())
    }
}

fn require_supported_tables_empty(connection: &rusqlite::Connection) -> Result<(), VaultError> {
    let unsupported: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_summaries)
             OR EXISTS(SELECT 1 FROM assessments)
             OR EXISTS(SELECT 1 FROM consent_records)",
        [],
        |row| row.get(0),
    )?;
    if unsupported {
        Err(VaultError::InvalidArtifact)
    } else {
        Ok(())
    }
}

fn require_user_portability_actor(actor: Actor, capability: Capability) -> Result<(), VaultError> {
    actor.require(capability)?;
    if actor.kind() == ActorKind::User {
        Ok(())
    } else {
        Err(quisum_core::DomainError::Forbidden(capability).into())
    }
}

fn parse_enum<T: DeserializeOwned>(token: &str) -> Result<T, VaultError> {
    serde_json::from_value(serde_json::Value::String(token.to_owned())).map_err(VaultError::from)
}

fn enum_display(value: &impl Serialize) -> Result<String, VaultError> {
    store::enum_token(value)
}

fn parse_time(value: &str) -> Result<DateTime<Utc>, VaultError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| VaultError::InvalidArtifact)
}

fn push_quoted(markdown: &mut String, value: &str) {
    for line in value.lines() {
        markdown.push_str("> ");
        markdown.push_str(line);
        markdown.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, error::Error, fs};

    use quisum_core::{
        ClaimKind, ContextBuilder, Event, EvidenceGrade, PrivacyKind, ProviderCapabilities,
        ProviderKind, ProviderProfile, ReflectionSession, ReviewDecision, ReviewRequest,
        Sensitivity, SessionMessage, Source,
    };
    use tempfile::tempdir;

    use super::*;
    use crate::VaultKey;

    fn populated_vault(path: &Path) -> Result<(SqlCipherVault, Event, Claim), VaultError> {
        let vault = SqlCipherVault::create(path, &VaultKey::generate()?)?;
        let session = ReflectionSession::user_started(
            "free-reflection@1.0.0",
            Some("Synthetic portable session".to_owned()),
            Sensitivity::Private,
            Utc::now(),
        );
        vault.create_session(Actor::user(), &session)?;
        vault.append_session_message(
            Actor::user(),
            &SessionMessage::user(
                session.id,
                "A portable synthetic user message.",
                Sensitivity::Private,
                Utc::now(),
            ),
        )?;
        let event = Event::user_confirmed(
            "Synthetic export event",
            vec!["A bounded synthetic observation.".to_owned()],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;
        let mut claim = Claim::proposal(
            ClaimKind::PatternHypothesis,
            "The synthetic situation may reward earlier clarification.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            Source::user(),
            Utc::now(),
        );
        claim.supporting_evidence.insert(event.id.clone());
        claim
            .alternative_explanations
            .push("The instructions may simply have been ambiguous.".to_owned());
        vault.insert_claim(Actor::user(), &claim)?;
        let reviewed = vault.review_claim(
            Actor::user(),
            &claim.id,
            ReviewRequest {
                decision: ReviewDecision::Confirm,
                edited_statement: None,
                note: Some("Synthetic review note.".to_owned()),
            },
            Utc::now(),
        )?;
        let confirmed = vault
            .get_claim(Actor::user(), &claim.id)?
            .ok_or(VaultError::NotFoundRecord)?;
        assert_eq!(reviewed.outcome.review.claim_id, claim.id);
        Ok((vault, event, confirmed))
    }

    #[test]
    fn jsonl_round_trip_preserves_claim_reviews_and_search() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let (source, event, claim) = populated_vault(&directory.path().join("source.db"))?;
        let export_path = directory.path().join("portable.jsonl");
        let receipt =
            source.export_portable(Actor::user(), &export_path, ExportFormat::Jsonl, Utc::now())?;
        assert!(receipt.record_count >= 5);
        let target =
            SqlCipherVault::create(directory.path().join("target.db"), &VaultKey::generate()?)?;
        let imported = target.import_portable(Actor::user(), &export_path, Utc::now())?;
        assert_eq!(imported.record_count, receipt.record_count);
        assert_eq!(
            target.get_claim(Actor::user(), &claim.id)?,
            Some(claim.clone())
        );
        assert_eq!(target.claim_revision_count(&claim.id)?, 1);
        assert_eq!(target.list_sessions(Actor::user(), 10)?.len(), 1);
        let imported_session = target
            .list_sessions(Actor::user(), 10)?
            .into_iter()
            .next()
            .ok_or(VaultError::NotFoundRecord)?;
        assert_eq!(
            target
                .list_session_messages(Actor::user(), &imported_session.id, 10)?
                .len(),
            1
        );
        assert!(
            target
                .search_memory(Actor::user(), "bounded observation", 10)?
                .iter()
                .any(|candidate| candidate.item_id == event.id)
        );
        Ok(())
    }

    #[test]
    fn json_round_trip_and_markdown_are_explicitly_distinct() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let (source, _, _) = populated_vault(&directory.path().join("source.db"))?;
        let export_path = directory.path().join("portable.json");
        source.export_portable(Actor::user(), &export_path, ExportFormat::Json, Utc::now())?;
        let target =
            SqlCipherVault::create(directory.path().join("target.db"), &VaultKey::generate()?)?;
        target.import_portable(Actor::user(), &export_path, Utc::now())?;

        let markdown_path = directory.path().join("WHOAMI.md");
        let markdown = source.export_markdown(Actor::user(), &markdown_path, Utc::now())?;
        assert_eq!(markdown.claim_count, 1);
        assert!(fs::read_to_string(markdown_path)?.contains("not a backup"));
        Ok(())
    }

    #[test]
    fn checksum_tampering_and_nonempty_targets_are_rejected_atomically()
    -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let (source, _, _) = populated_vault(&directory.path().join("source.db"))?;
        let export_path = directory.path().join("portable.jsonl");
        source.export_portable(Actor::user(), &export_path, ExportFormat::Jsonl, Utc::now())?;
        let target =
            SqlCipherVault::create(directory.path().join("target.db"), &VaultKey::generate()?)?;
        let existing = Event::user_confirmed(
            "Existing",
            vec!["Must survive failed import.".to_owned()],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        target.insert_event(Actor::user(), &existing)?;
        assert!(matches!(
            target.import_portable(Actor::user(), &export_path, Utc::now()),
            Err(VaultError::Conflict)
        ));
        assert!(
            target
                .search_memory(Actor::user(), "survive failed import", 10)?
                .iter()
                .any(|candidate| candidate.item_id == existing.id)
        );

        let mut bytes = fs::read(&export_path)?;
        let last = bytes
            .len()
            .checked_sub(2)
            .ok_or(VaultError::InvalidArtifact)?;
        bytes[last] ^= 1;
        fs::write(&export_path, bytes)?;
        let empty =
            SqlCipherVault::create(directory.path().join("empty.db"), &VaultKey::generate()?)?;
        assert!(matches!(
            empty.import_portable(Actor::user(), &export_path, Utc::now()),
            Err(VaultError::ArtifactTampered)
                | Err(VaultError::Serialization(_))
                | Err(VaultError::InvalidArtifact)
        ));
        assert!(
            empty
                .search_memory(Actor::user(), "synthetic", 10)?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn imported_provider_profiles_are_disabled_and_cannot_alias_existing_credentials()
    -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let source =
            SqlCipherVault::create(directory.path().join("source.db"), &VaultKey::generate()?)?;
        let profile = ProviderProfile::new(
            "Synthetic credential isolation",
            ProviderKind::OpenaiCompatible,
            "https://models.example.invalid/v1",
            "synthetic-model",
            PrivacyKind::Cloud,
            ProviderCapabilities {
                streaming: false,
                tool_use: false,
                json_schema: false,
                vision: false,
                context_limit: Some(8_192),
            },
            BTreeSet::from([Sensitivity::Normal]),
            Some("provider.synthetic-existing-secret".to_owned()),
            None,
            Utc::now(),
        );
        source.put_provider_profile(Actor::user(), &profile)?;
        let export_path = directory.path().join("portable.jsonl");
        source.export_portable(Actor::user(), &export_path, ExportFormat::Jsonl, Utc::now())?;

        let target =
            SqlCipherVault::create(directory.path().join("target.db"), &VaultKey::generate()?)?;
        target.import_portable(Actor::user(), &export_path, Utc::now())?;
        let imported = target
            .get_provider_profile(Actor::user(), &profile.id)?
            .ok_or(VaultError::NotFoundRecord)?;
        assert!(!imported.enabled);
        assert_ne!(imported.credential_ref, profile.credential_ref);
        assert!(
            imported
                .credential_ref
                .as_deref()
                .is_some_and(|reference| reference.starts_with("import.import_"))
        );
        Ok(())
    }

    #[test]
    fn portable_graph_rejects_forged_provider_provenance_and_duplicate_assistant_turns()
    -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        let profile = ProviderProfile::new(
            "Synthetic portable provider",
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
        let mut packet = ContextBuilder::new(1)?.build(
            Actor::user(),
            "Synthetic portable provenance",
            "free-reflection@1.0.0",
            profile.id.clone(),
            profile.model.clone(),
            "A synthetic user turn",
            Sensitivity::Private,
            Vec::new(),
            now,
        )?;
        packet.mark_dispatched(Actor::user(), now)?;
        let forged_event = Event::proposed(
            "Synthetic forged event",
            vec!["A synthetic observation.".to_owned()],
            now,
            Sensitivity::Private,
            Source::model(
                packet.method.clone(),
                "forged-provider",
                packet.model.clone(),
                packet.id.clone(),
            ),
        );
        assert!(matches!(
            validate_record_graph(&[
                PortableRecord::ProviderProfile(profile.clone()),
                PortableRecord::ContextPacket(packet.clone()),
                PortableRecord::Event(forged_event),
            ]),
            Err(VaultError::InvalidArtifact)
        ));

        let mut unreviewed_model_claim = Claim::proposal(
            ClaimKind::ExplicitFact,
            "A model claim cannot forge a human-confirmed state.",
            EvidenceGrade::Limited,
            Sensitivity::Private,
            Source::model(
                packet.method.clone(),
                profile.name.clone(),
                packet.model.clone(),
                packet.id.clone(),
            ),
            now,
        );
        unreviewed_model_claim.status = ClaimStatus::Confirmed;
        unreviewed_model_claim.user_endorsement = Some(UserEndorsement::Accepted);
        assert!(matches!(
            validate_record_graph(&[
                PortableRecord::ProviderProfile(profile.clone()),
                PortableRecord::ContextPacket(packet.clone()),
                PortableRecord::Claim(unreviewed_model_claim),
            ]),
            Err(VaultError::InvalidArtifact)
        ));

        let reviewed_event = Event::user_confirmed(
            "Synthetic portable excerpt source",
            vec!["The actual reviewed observation.".to_owned()],
            None,
            now,
            Sensitivity::Private,
        );
        let mut forged_packet = ContextBuilder::new(1)?.build(
            Actor::user(),
            "Synthetic forged excerpt",
            "event-debrief@1.0.0",
            profile.id.clone(),
            profile.model.clone(),
            "A synthetic query",
            Sensitivity::Private,
            vec![quisum_core::MemoryCandidate {
                item_type: ContextItemType::Event,
                item_id: reviewed_event.id.clone(),
                excerpt: "A different, fabricated observation.".to_owned(),
                sensitivity: Sensitivity::Private,
                state: quisum_core::MemoryState::Confirmed,
                crisis_related: false,
                relevant: true,
                policy_allowed: true,
                selection_reason: "synthetic_forgery".to_owned(),
                relevance_score: 1_000,
                recorded_at: now,
            }],
            now,
        )?;
        forged_packet.mark_dispatched(Actor::user(), now)?;
        assert!(matches!(
            validate_record_graph(&[
                PortableRecord::ProviderProfile(profile.clone()),
                PortableRecord::ContextPacket(forged_packet),
                PortableRecord::Event(reviewed_event),
            ]),
            Err(VaultError::InvalidArtifact)
        ));

        let session =
            ReflectionSession::user_started(packet.method.clone(), None, Sensitivity::Private, now);
        let user = SessionMessage::user(
            session.id.clone(),
            packet.query.clone(),
            Sensitivity::Private,
            now,
        );
        let source = Source::model(
            packet.method.clone(),
            profile.name.clone(),
            packet.model.clone(),
            packet.id.clone(),
        );
        let first = SessionMessage::assistant(
            session.id.clone(),
            "First bounded synthetic response.",
            Sensitivity::Private,
            source.clone(),
            now + chrono::Duration::seconds(1),
        );
        let duplicate = SessionMessage::assistant(
            session.id.clone(),
            "Duplicate response without a new user turn.",
            Sensitivity::Private,
            source,
            now + chrono::Duration::seconds(2),
        );
        assert!(matches!(
            validate_record_graph(&[
                PortableRecord::ProviderProfile(profile),
                PortableRecord::ContextPacket(packet),
                PortableRecord::Session(session),
                PortableRecord::Message(user),
                PortableRecord::Message(first),
                PortableRecord::Message(duplicate),
            ]),
            Err(VaultError::InvalidArtifact)
        ));
        Ok(())
    }

    #[test]
    fn portable_graph_cannot_bypass_runtime_safety_gates() -> Result<(), Box<dyn Error>> {
        let now = Utc::now();
        let unsafe_user_event = Event::user_confirmed(
            "Immediate danger",
            vec!["I have a weapon and plan to hurt myself tonight.".to_owned()],
            None,
            now,
            Sensitivity::Private,
        );
        assert!(matches!(
            validate_record_graph(&[PortableRecord::Event(unsafe_user_event)]),
            Err(VaultError::Domain(quisum_core::DomainError::SafetyBlocked))
        ));

        let profile = ProviderProfile::new(
            "Synthetic safety provider",
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
        let mut packet = ContextBuilder::new(1)?.build(
            Actor::user(),
            "Synthetic safety provenance",
            "free-reflection@1.0.0",
            profile.id.clone(),
            profile.model.clone(),
            "A safe synthetic user turn",
            Sensitivity::Private,
            Vec::new(),
            now,
        )?;
        packet.mark_dispatched(Actor::user(), now)?;
        let session =
            ReflectionSession::user_started(packet.method.clone(), None, Sensitivity::Private, now);
        let user = SessionMessage::user(
            session.id.clone(),
            packet.query.clone(),
            Sensitivity::Private,
            now,
        );
        let assistant = SessionMessage::assistant(
            session.id.clone(),
            "Only I understand you.",
            Sensitivity::Private,
            Source::model(
                packet.method.clone(),
                profile.name.clone(),
                packet.model.clone(),
                packet.id.clone(),
            ),
            now + chrono::Duration::milliseconds(1),
        );
        assert!(matches!(
            validate_record_graph(&[
                PortableRecord::ProviderProfile(profile),
                PortableRecord::ContextPacket(packet),
                PortableRecord::Session(session),
                PortableRecord::Message(user),
                PortableRecord::Message(assistant),
            ]),
            Err(VaultError::Domain(quisum_core::DomainError::SafetyBlocked))
        ));
        Ok(())
    }
}
