#![doc = "Named Tauri command adapter for the Quisum desktop client."]
#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use quisum_core::{
    Actor, BehavioralExperiment, Claim, ClaimKind, ContextBuilder, CrisisGuidance,
    CrisisResourcePack, DomainError, EntityId, Event, EvidenceGrade, ExperimentRisk, MethodError,
    MethodPackage, OFFICIAL_METHOD_IDS, OpenQuestionStatus, PrivacyKind, ProviderCapabilities,
    ProviderKind, ProviderProfile, ReflectionSession, ReviewDecision, ReviewRequest, SafetyEngine,
    SafetyRoute, Sensitivity, SessionMessage, Source, Validate,
};
use quisum_providers::{
    GenerationRequest, HttpProvider, ModelProvider as _, OutputContract, ProviderCredential,
    ProviderError,
};
use quisum_vault::{
    ExportFormat, OsSecretStore, RecoveryKeyEnvelope, RestoredVault, SqlCipherVault, VaultError,
    VaultKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{Manager as _, State};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

const MAX_RECOVERY_FILE_BYTES: u64 = 64 * 1_024;
const MAX_PATH_CHARACTERS: usize = 4_096;

type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Clone)]
struct DesktopPaths {
    vault: PathBuf,
    recovery: PathBuf,
    methods: PathBuf,
}

struct DesktopState {
    paths: DesktopPaths,
    unlocked: Mutex<Option<SqlCipherVault>>,
}

impl DesktopState {
    fn new(paths: DesktopPaths) -> Self {
        Self {
            paths,
            unlocked: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    crisis_guidance: Option<Box<CrisisGuidance>>,
}

impl CommandError {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            crisis_guidance: None,
        }
    }

    fn input(message: &str) -> Self {
        Self::new("invalid_input", message)
    }

    fn locked() -> Self {
        Self::new("vault_locked", "Unlock the encrypted vault to continue.")
    }

    fn crisis(guidance: CrisisGuidance) -> Self {
        Self {
            code: "crisis_route".to_owned(),
            message: guidance.assistant_response.clone(),
            crisis_guidance: Some(Box::new(guidance)),
        }
    }
}

impl From<DomainError> for CommandError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::SafetyBlocked => Self::new(
                "safety_route",
                "Quisum paused identity analysis and durable persistence. If anyone may be in immediate danger, contact local emergency services and a trusted person who can help without putting themselves in danger.",
            ),
            other => Self::new("domain_policy", &other.to_string()),
        }
    }
}

impl From<VaultError> for CommandError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::Domain(domain) => domain.into(),
            VaultError::AlreadyExists => Self::new(
                "already_exists",
                "Quisum never overwrites an existing artifact.",
            ),
            VaultError::NotFound | VaultError::NotFoundRecord => Self::new(
                "not_found",
                "The requested local record or artifact was not found.",
            ),
            VaultError::InvalidKeyOrCorrupt => Self::new(
                "unlock_failed",
                "The vault key is invalid or the encrypted vault is corrupt.",
            ),
            VaultError::CipherUnavailable => Self::new(
                "cipher_unavailable",
                "This build does not provide the required SQLCipher support.",
            ),
            VaultError::IntegrityCheckFailed
            | VaultError::MigrationChecksumMismatch { .. }
            | VaultError::MigrationFailed { .. } => Self::new(
                "integrity_failed",
                "The encrypted vault failed an integrity or migration check.",
            ),
            VaultError::UnsupportedSchema { .. } => Self::new(
                "unsupported_schema",
                "This vault was created by a newer unsupported Quisum version.",
            ),
            VaultError::RecoveryFailed => Self::new(
                "recovery_failed",
                "The passphrase is wrong or the authenticated recovery material was modified.",
            ),
            VaultError::InvalidRecoveryEnvelope => Self::new(
                "invalid_recovery",
                "The recovery material is invalid or exceeds safe work limits.",
            ),
            VaultError::SecretStoreUnavailable => Self::new(
                "secret_store_unavailable",
                "The operating-system credential store is unavailable.",
            ),
            VaultError::SecretNotFound => Self::new(
                "secret_not_found",
                "The requested credential was not found.",
            ),
            VaultError::InvalidArtifact => Self::new(
                "invalid_artifact",
                "The selected artifact is invalid or too large.",
            ),
            VaultError::ArtifactTampered => Self::new(
                "artifact_tampered",
                "The authenticated artifact was modified or is corrupt.",
            ),
            VaultError::Conflict => Self::new(
                "conflict",
                "The record changed or already exists; reload before trying again.",
            ),
            VaultError::RandomUnavailable => Self::new(
                "random_unavailable",
                "Secure random generation is unavailable on this device.",
            ),
            VaultError::Serialization(_)
            | VaultError::Database(_)
            | VaultError::FileSystem { .. } => Self::new(
                "local_operation_failed",
                "A local encrypted-storage or filesystem operation failed.",
            ),
        }
    }
}

impl From<ProviderError> for CommandError {
    fn from(error: ProviderError) -> Self {
        Self::new("provider_failed", &error.to_string())
    }
}

impl From<MethodError> for CommandError {
    fn from(error: MethodError) -> Self {
        Self::new("method_invalid", &error.to_string())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    product: &'static str,
    version: &'static str,
    vault_exists: bool,
    recovery_exists: bool,
    unlocked: bool,
    vault_path: PathBuf,
    recovery_path: PathBuf,
    encrypted: bool,
    telemetry: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateVaultInput {
    recovery_passphrase: String,
    recovery_confirmation: String,
    save_to_keyring: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UnlockRecoveryInput {
    recovery_passphrase: String,
    save_to_keyring: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LimitInput {
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdInput {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaptureEventInput {
    title: String,
    facts: Vec<String>,
    interpretations: Vec<String>,
    emotions: Vec<String>,
    bodily_responses: Vec<String>,
    actions: Vec<String>,
    short_term_outcomes: Vec<String>,
    long_term_outcomes: Vec<String>,
    topics: Vec<String>,
    sensitivity: String,
    occurred_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewEventInput {
    id: String,
    accept: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateClaimInput {
    kind: String,
    statement: String,
    contexts: Vec<String>,
    supporting_evidence: Vec<String>,
    counterevidence: Vec<String>,
    alternatives: Vec<String>,
    evidence_grade: String,
    sensitivity: String,
    revisit_after: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewClaimInput {
    id: String,
    decision: String,
    edited_statement: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CounterevidenceInput {
    claim_id: String,
    evidence_id: String,
    action: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewQuestionInput {
    id: String,
    target: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateExperimentInput {
    title: String,
    hypothesis: String,
    prediction: String,
    action: String,
    stop_condition: String,
    risk: String,
    related_claims: Vec<String>,
    expected_result: Option<String>,
    sensitivity: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransitionExperimentInput {
    id: String,
    action: String,
    actual_result: Option<String>,
    learning: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionDetailInput {
    id: String,
    message_limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransitionSessionInput {
    id: String,
    action: String,
    confirm_delete: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchInput {
    query: String,
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateProviderInput {
    name: String,
    kind: String,
    base_url: String,
    model: String,
    privacy_kind: String,
    trusted_sensitivities: Vec<String>,
    streaming: bool,
    tool_use: bool,
    json_schema: bool,
    vision: bool,
    context_limit: Option<u32>,
    retention_note: Option<String>,
    credential: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToggleProviderInput {
    id: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextPreviewInput {
    provider_id: String,
    purpose: String,
    method: String,
    query: String,
    query_sensitivity: String,
    maximum_items: usize,
    region: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExploreSendInput {
    context_id: String,
    session_id: Option<String>,
    title: Option<String>,
    max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportInput {
    destination: String,
    format: String,
    acknowledge_plaintext: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportInput {
    source: String,
    confirm_empty_vault_import: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BackupInput {
    destination: String,
    recovery_passphrase: String,
    recovery_confirmation: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreBackupInput {
    source: String,
    recovery_passphrase: String,
    save_to_keyring: bool,
}

#[tauri::command]
async fn app_status(state: State<'_, DesktopState>) -> CommandResult<AppStatus> {
    let unlocked = state.unlocked.lock().await.is_some();
    Ok(AppStatus {
        product: quisum_core::PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        vault_exists: state.paths.vault.is_file(),
        recovery_exists: state.paths.recovery.is_file(),
        unlocked,
        vault_path: state.paths.vault.clone(),
        recovery_path: state.paths.recovery.clone(),
        encrypted: true,
        telemetry: false,
    })
}

#[tauri::command]
async fn vault_create(
    state: State<'_, DesktopState>,
    input: CreateVaultInput,
) -> CommandResult<Value> {
    let passphrase = Zeroizing::new(input.recovery_passphrase);
    let confirmation = Zeroizing::new(input.recovery_confirmation);
    if passphrase.as_str() != confirmation.as_str() {
        return Err(CommandError::input(
            "The recovery passphrases do not match.",
        ));
    }
    let mut unlocked = state.unlocked.lock().await;
    if unlocked.is_some() || state.paths.vault.exists() || state.paths.recovery.exists() {
        return Err(CommandError::new(
            "already_exists",
            "A vault or recovery artifact already exists; Quisum never overwrites it.",
        ));
    }

    let key = VaultKey::generate()?;
    let envelope = RecoveryKeyEnvelope::seal(&key, &passphrase)?;
    write_private_json_new(&state.paths.recovery, &envelope)?;
    let vault = match SqlCipherVault::create(&state.paths.vault, &key) {
        Ok(vault) => vault,
        Err(error) => {
            remove_file_if_present(&state.paths.recovery);
            return Err(error.into());
        }
    };
    let vault_id = match vault.vault_id() {
        Ok(id) => id,
        Err(error) => {
            drop(vault);
            cleanup_vault_files(&state.paths.vault);
            remove_file_if_present(&state.paths.recovery);
            return Err(error.into());
        }
    };
    let keyring_saved = input.save_to_keyring
        && OsSecretStore
            .put_vault_key(&state.paths.vault, &key)
            .is_ok();
    *unlocked = Some(vault);
    Ok(json!({
        "vaultId": vault_id,
        "vaultPath": state.paths.vault,
        "recoveryPath": state.paths.recovery,
        "keyringSaved": keyring_saved
    }))
}

#[tauri::command]
async fn vault_unlock_keyring(state: State<'_, DesktopState>) -> CommandResult<Value> {
    let mut unlocked = state.unlocked.lock().await;
    if let Some(vault) = unlocked.as_ref() {
        return vault_summary(vault);
    }
    let key = OsSecretStore.get_vault_key(&state.paths.vault)?;
    let vault = SqlCipherVault::open(&state.paths.vault, &key)?;
    let summary = vault_summary(&vault)?;
    *unlocked = Some(vault);
    Ok(summary)
}

#[tauri::command]
async fn vault_unlock_recovery(
    state: State<'_, DesktopState>,
    input: UnlockRecoveryInput,
) -> CommandResult<Value> {
    let passphrase = Zeroizing::new(input.recovery_passphrase);
    let mut unlocked = state.unlocked.lock().await;
    if let Some(vault) = unlocked.as_ref() {
        return vault_summary(vault);
    }
    let envelope = read_recovery_envelope(&state.paths.recovery)?;
    let key = envelope.open(&passphrase)?;
    let vault = SqlCipherVault::open(&state.paths.vault, &key)?;
    let keyring_saved = input.save_to_keyring
        && OsSecretStore
            .put_vault_key(&state.paths.vault, &key)
            .is_ok();
    let mut summary = vault_summary(&vault)?;
    summary["keyringSaved"] = Value::Bool(keyring_saved);
    *unlocked = Some(vault);
    Ok(summary)
}

#[tauri::command]
async fn vault_lock(state: State<'_, DesktopState>) -> CommandResult<Value> {
    let mut unlocked = state.unlocked.lock().await;
    if let Some(vault) = unlocked.take() {
        vault.close()?;
    }
    Ok(json!({"locked": true}))
}

#[tauri::command]
async fn vault_doctor(state: State<'_, DesktopState>) -> CommandResult<Value> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    vault.integrity_check()?;
    Ok(json!({
        "ok": true,
        "sqlcipher": "verified",
        "integrity": "ok",
        "schemaVersion": vault.schema_version()?,
        "latestSupportedSchema": quisum_vault::LATEST_SCHEMA_VERSION,
        "recoveryEnvelopePresent": state.paths.recovery.is_file(),
        "telemetry": false
    }))
}

#[tauri::command]
async fn method_list(state: State<'_, DesktopState>) -> CommandResult<Vec<Value>> {
    OFFICIAL_METHOD_IDS
        .iter()
        .map(|method| {
            let package = MethodPackage::load(&state.paths.methods, method)?;
            Ok(json!({
                "id": package.identifier(),
                "title": package.manifest.title,
                "description": package.manifest.description,
                "reviewStatus": package.manifest.review.status,
                "requires": package.manifest.requires,
                "valid": true
            }))
        })
        .collect()
}

#[tauri::command]
async fn event_list(
    state: State<'_, DesktopState>,
    input: LimitInput,
) -> CommandResult<Vec<Event>> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault.list_events(Actor::user(), input.limit)?)
}

#[tauri::command]
async fn event_capture(
    state: State<'_, DesktopState>,
    input: CaptureEventInput,
) -> CommandResult<Event> {
    require_safe_input(
        &joined_user_text(
            &input.title,
            [
                &input.facts,
                &input.interpretations,
                &input.emotions,
                &input.bodily_responses,
                &input.actions,
                &input.short_term_outcomes,
                &input.long_term_outcomes,
                &input.topics,
            ],
        ),
        None,
    )?;
    let now = Utc::now();
    let mut event = Event::user_confirmed(
        input.title,
        input.facts,
        parse_optional_time(input.occurred_at.as_deref())?,
        now,
        parse_sensitivity(&input.sensitivity)?,
    );
    event.interpretations = input.interpretations;
    event.emotions = input.emotions;
    event.bodily_responses = input.bodily_responses;
    event.actions = input.actions;
    event.short_term_outcomes = input.short_term_outcomes;
    event.long_term_outcomes = input.long_term_outcomes;
    event.topics = input.topics.into_iter().collect();
    event.validate()?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    vault.insert_event(Actor::user(), &event)?;
    Ok(event)
}

#[tauri::command]
async fn event_review(
    state: State<'_, DesktopState>,
    input: ReviewEventInput,
) -> CommandResult<Event> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault.review_event(Actor::user(), &id, input.accept, Utc::now())?)
}

#[tauri::command]
async fn event_delete(state: State<'_, DesktopState>, input: IdInput) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let impact = vault.delete_event(Actor::user(), &id, Utc::now())?;
    Ok(json!({
        "deletedEvent": impact.event_id,
        "affectedClaims": impact.affected_claims,
        "purgedPendingClaims": impact.purged_pending_claims
    }))
}

#[tauri::command]
async fn claim_list(
    state: State<'_, DesktopState>,
    input: LimitInput,
) -> CommandResult<Vec<Claim>> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault.list_claims(Actor::user(), input.limit)?)
}

#[tauri::command]
async fn claim_review_queue(
    state: State<'_, DesktopState>,
    input: LimitInput,
) -> CommandResult<Vec<Claim>> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault.list_review_claims(Actor::user(), input.limit)?)
}

#[tauri::command]
async fn claim_create(
    state: State<'_, DesktopState>,
    input: CreateClaimInput,
) -> CommandResult<Claim> {
    require_safe_input(
        &joined_user_text(&input.statement, [&input.contexts, &input.alternatives]),
        None,
    )?;
    let now = Utc::now();
    let mut claim = Claim::proposal(
        parse_claim_kind(&input.kind)?,
        input.statement,
        parse_evidence_grade(&input.evidence_grade)?,
        parse_sensitivity(&input.sensitivity)?,
        Source::user(),
        now,
    );
    claim.contexts = input.contexts.into_iter().collect();
    claim.supporting_evidence = parse_id_set(input.supporting_evidence)?;
    claim.counterevidence = parse_id_set(input.counterevidence)?;
    claim.alternative_explanations = input.alternatives;
    claim.revisit_after = parse_optional_time(input.revisit_after.as_deref())?;
    claim.validate()?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    vault.insert_claim(Actor::user(), &claim)?;
    Ok(claim)
}

#[tauri::command]
async fn claim_review(
    state: State<'_, DesktopState>,
    input: ReviewClaimInput,
) -> CommandResult<Value> {
    require_safe_input(
        &joined_optional_user_text([input.edited_statement.as_deref(), input.note.as_deref()]),
        None,
    )?;
    let id = parse_id(&input.id)?;
    let request = ReviewRequest {
        decision: parse_review_decision(&input.decision)?,
        edited_statement: nonempty_option(input.edited_statement),
        note: nonempty_option(input.note),
    };
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let commit = vault.review_claim(Actor::user(), &id, request, Utc::now())?;
    Ok(json!({
        "review": commit.outcome.review,
        "revision": commit.outcome.revision,
        "openQuestion": commit.open_question
    }))
}

#[tauri::command]
async fn claim_counterevidence(
    state: State<'_, DesktopState>,
    input: CounterevidenceInput,
) -> CommandResult<Value> {
    let claim_id = parse_id(&input.claim_id)?;
    let evidence_id = parse_id(&input.evidence_id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    match input.action.as_str() {
        "propose" => {
            let proposal = vault.propose_counterevidence(
                Actor::user(),
                &claim_id,
                &evidence_id,
                None,
                Utc::now(),
            )?;
            Ok(json!({
                "claimId": proposal.claim_id,
                "evidenceId": proposal.evidence_id,
                "status": "proposed",
                "createdAt": proposal.created_at
            }))
        }
        "accept" => Ok(json!(vault.accept_counterevidence(
            Actor::user(),
            &claim_id,
            &evidence_id,
            Utc::now(),
        )?)),
        _ => Err(CommandError::input(
            "Counterevidence action must be propose or accept.",
        )),
    }
}

#[tauri::command]
async fn question_list(state: State<'_, DesktopState>, input: LimitInput) -> CommandResult<Value> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(
        vault.list_open_questions(Actor::user(), input.limit)?
    ))
}

#[tauri::command]
async fn question_review(
    state: State<'_, DesktopState>,
    input: ReviewQuestionInput,
) -> CommandResult<Value> {
    let target = match input.target.as_str() {
        "open" => OpenQuestionStatus::Open,
        "answered" => OpenQuestionStatus::Answered,
        "closed" => OpenQuestionStatus::Closed,
        _ => {
            return Err(CommandError::input(
                "Question target must be open, answered, or closed.",
            ));
        }
    };
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(vault.review_open_question(
        Actor::user(),
        &id,
        target,
        Utc::now(),
    )?))
}

#[tauri::command]
async fn experiment_list(
    state: State<'_, DesktopState>,
    input: LimitInput,
) -> CommandResult<Value> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(vault.list_experiments(Actor::user(), input.limit)?))
}

#[tauri::command]
async fn experiment_create(
    state: State<'_, DesktopState>,
    input: CreateExperimentInput,
) -> CommandResult<Value> {
    let mut safety_text = joined_user_text(
        &input.title,
        [
            std::slice::from_ref(&input.hypothesis),
            std::slice::from_ref(&input.prediction),
            std::slice::from_ref(&input.action),
            std::slice::from_ref(&input.stop_condition),
        ],
    );
    append_user_text(&mut safety_text, input.expected_result.as_deref());
    require_safe_input(&safety_text, None)?;
    let mut experiment = BehavioralExperiment::proposal(
        input.title,
        input.hypothesis,
        input.prediction,
        input.action,
        input.stop_condition,
        parse_experiment_risk(&input.risk)?,
        Utc::now(),
        parse_sensitivity(&input.sensitivity)?,
        Source::user(),
    );
    experiment.related_claims = parse_id_set(input.related_claims)?;
    experiment.expected_result = nonempty_option(input.expected_result);
    experiment.validate()?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    vault.insert_experiment(Actor::user(), &experiment)?;
    Ok(json!(experiment))
}

#[tauri::command]
async fn experiment_transition(
    state: State<'_, DesktopState>,
    input: TransitionExperimentInput,
) -> CommandResult<Value> {
    require_safe_input(
        &joined_optional_user_text([input.actual_result.as_deref(), input.learning.as_deref()]),
        None,
    )?;
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let experiment = match input.action.as_str() {
        "activate" => vault.activate_experiment(Actor::user(), &id, Utc::now())?,
        "complete" => vault.complete_experiment(
            Actor::user(),
            &id,
            required_option(input.actual_result, "Completion requires an actual result.")?,
            required_option(input.learning, "Completion requires a learning note.")?,
            Utc::now(),
        )?,
        "cancel" => vault.cancel_experiment(Actor::user(), &id, Utc::now())?,
        _ => {
            return Err(CommandError::input(
                "Experiment action must be activate, complete, or cancel.",
            ));
        }
    };
    Ok(json!(experiment))
}

#[tauri::command]
async fn session_list(state: State<'_, DesktopState>, input: LimitInput) -> CommandResult<Value> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(vault.list_sessions(Actor::user(), input.limit)?))
}

#[tauri::command]
async fn session_detail(
    state: State<'_, DesktopState>,
    input: SessionDetailInput,
) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let session = vault
        .get_session(Actor::user(), &id)?
        .ok_or_else(|| CommandError::new("not_found", "The session was not found."))?;
    let messages = vault.list_session_messages(Actor::user(), &id, input.message_limit)?;
    Ok(json!({"session": session, "messages": messages}))
}

#[tauri::command]
async fn session_transition(
    state: State<'_, DesktopState>,
    input: TransitionSessionInput,
) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    match input.action.as_str() {
        "complete" => Ok(json!(vault.close_session(
            Actor::user(),
            &id,
            true,
            Utc::now(),
        )?)),
        "abandon" => Ok(json!(vault.close_session(
            Actor::user(),
            &id,
            false,
            Utc::now(),
        )?)),
        "delete" if input.confirm_delete => {
            let impact = vault.delete_session(Actor::user(), &id, Utc::now())?;
            Ok(json!({
                "deletedSession": impact.session_id,
                "deletedMessages": impact.deleted_messages,
                "affectedClaims": impact.affected_claims,
                "purgedPendingClaims": impact.purged_pending_claims
            }))
        }
        "delete" => Err(CommandError::input(
            "Session deletion requires explicit confirmation.",
        )),
        _ => Err(CommandError::input(
            "Session action must be complete, abandon, or delete.",
        )),
    }
}

#[tauri::command]
async fn memory_search(
    state: State<'_, DesktopState>,
    input: SearchInput,
) -> CommandResult<Vec<Value>> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault
        .search_memory(Actor::user(), &input.query, input.limit)?
        .into_iter()
        .map(|candidate| {
            json!({
                "itemType": candidate.item_type,
                "itemId": candidate.item_id,
                "excerpt": candidate.excerpt,
                "sensitivity": candidate.sensitivity,
                "selectionReason": candidate.selection_reason,
                "relevanceScore": candidate.relevance_score,
                "recordedAt": candidate.recorded_at
            })
        })
        .collect())
}

#[tauri::command]
async fn reflog_list(
    state: State<'_, DesktopState>,
    input: LimitInput,
) -> CommandResult<Vec<Value>> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault
        .list_reflog(Actor::user(), input.limit)?
        .into_iter()
        .map(|entry| json!({"sequence": entry.sequence, "event": entry.event}))
        .collect())
}

#[tauri::command]
async fn provider_list(state: State<'_, DesktopState>) -> CommandResult<Vec<ProviderProfile>> {
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(vault.list_provider_profiles(Actor::user())?)
}

#[tauri::command]
async fn provider_create(
    state: State<'_, DesktopState>,
    input: CreateProviderInput,
) -> CommandResult<ProviderProfile> {
    let credential = Zeroizing::new(input.credential);
    let privacy_kind = parse_privacy_kind(&input.privacy_kind)?;
    let needs_secret = privacy_kind == PrivacyKind::Cloud || !credential.trim().is_empty();
    if privacy_kind == PrivacyKind::Cloud && credential.trim().is_empty() {
        return Err(CommandError::input(
            "Cloud provider profiles require an API credential.",
        ));
    }
    let now = Utc::now();
    let mut profile = ProviderProfile::new(
        input.name,
        parse_provider_kind(&input.kind)?,
        input.base_url,
        input.model,
        privacy_kind,
        ProviderCapabilities {
            streaming: input.streaming,
            tool_use: input.tool_use,
            json_schema: input.json_schema,
            vision: input.vision,
            context_limit: input.context_limit,
        },
        input
            .trusted_sensitivities
            .iter()
            .map(|value| parse_sensitivity(value))
            .collect::<CommandResult<BTreeSet<_>>>()?,
        None,
        nonempty_option(input.retention_note),
        now,
    );
    if needs_secret {
        profile.credential_ref = Some(format!("provider.{}", profile.id).to_ascii_lowercase());
    }
    profile.validate()?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    if let Some(reference) = &profile.credential_ref {
        OsSecretStore.put_provider_secret(reference, credential.as_bytes())?;
    }
    if let Err(error) = vault.put_provider_profile(Actor::user(), &profile) {
        if let Some(reference) = &profile.credential_ref {
            ignore_secret_delete(reference);
        }
        return Err(error.into());
    }
    Ok(profile)
}

#[tauri::command]
async fn provider_toggle(
    state: State<'_, DesktopState>,
    input: ToggleProviderInput,
) -> CommandResult<ProviderProfile> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let mut profile = vault
        .get_provider_profile(Actor::user(), &id)?
        .ok_or_else(|| CommandError::new("not_found", "The provider profile was not found."))?;
    if input.enabled {
        if let Some(reference) = profile.credential_ref.as_deref() {
            let credential_check = OsSecretStore.get_provider_secret(reference)?;
            drop(credential_check);
        }
    }
    if profile.enabled != input.enabled {
        profile.enabled = input.enabled;
        profile.updated_at = next_timestamp(profile.updated_at)?;
        vault.put_provider_profile(Actor::user(), &profile)?;
    }
    Ok(profile)
}

#[tauri::command]
async fn provider_revoke(state: State<'_, DesktopState>, input: IdInput) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let mut profile = vault
        .get_provider_profile(Actor::user(), &id)?
        .ok_or_else(|| CommandError::new("not_found", "The provider profile was not found."))?;
    let reference = profile
        .credential_ref
        .as_deref()
        .ok_or_else(|| CommandError::input("This provider has no stored credential."))?
        .to_owned();
    if profile.enabled {
        profile.enabled = false;
        profile.updated_at = next_timestamp(profile.updated_at)?;
        vault.put_provider_profile(Actor::user(), &profile)?;
    }
    match OsSecretStore.delete_provider_secret(&reference) {
        Ok(()) | Err(VaultError::SecretNotFound) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(json!({"profile": profile, "secretRevoked": true}))
}

#[tauri::command]
async fn provider_test(state: State<'_, DesktopState>, input: IdInput) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let profile = vault
        .get_provider_profile(Actor::user(), &id)?
        .ok_or_else(|| CommandError::new("not_found", "The provider profile was not found."))?;
    let adapter = provider_adapter(profile.clone())?;
    let tested = adapter.test_connection().await;
    vault.record_provider_test(Actor::user(), &profile.id, tested.is_ok(), Utc::now())?;
    let tested = tested?;
    Ok(json!({
        "succeeded": tested.succeeded,
        "latencyMs": u64::try_from(tested.latency.as_millis()).unwrap_or(u64::MAX)
    }))
}

#[tauri::command]
async fn context_preview(
    state: State<'_, DesktopState>,
    input: ContextPreviewInput,
) -> CommandResult<Value> {
    require_safe_input(
        &joined_user_text(&input.query, [std::slice::from_ref(&input.purpose)]),
        input.region.as_deref(),
    )?;
    let profile_id = parse_id(&input.provider_id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let profile = vault
        .get_provider_profile(Actor::user(), &profile_id)?
        .ok_or_else(|| CommandError::new("not_found", "The provider profile was not found."))?;
    if !profile.enabled {
        return Err(CommandError::input("The selected provider is disabled."));
    }
    let package = MethodPackage::load(&state.paths.methods, &input.method)?;
    if !package.supports(&profile.capabilities) {
        return Err(CommandError::input(
            "The provider does not declare every capability required by this method.",
        ));
    }
    let query_sensitivity = parse_sensitivity(&input.query_sensitivity)?;
    if !profile.trusts(query_sensitivity) {
        return Err(CommandError::input(
            "The provider profile does not permit the request sensitivity.",
        ));
    }
    let mut candidates = vault.search_memory(Actor::user(), &input.query, 100)?;
    for candidate in &mut candidates {
        candidate.policy_allowed = profile.trusts(candidate.sensitivity);
    }
    let packet = ContextBuilder::new(input.maximum_items)?.build(
        Actor::user(),
        input.purpose,
        package.identifier(),
        profile.id,
        profile.model,
        input.query,
        query_sensitivity,
        candidates,
        Utc::now(),
    )?;
    vault.insert_context_packet(Actor::system(), &packet)?;
    Ok(json!(packet))
}

#[tauri::command]
async fn context_approve(state: State<'_, DesktopState>, input: IdInput) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(vault.approve_context_packet(
        Actor::user(),
        &id,
        Utc::now(),
    )?))
}

#[tauri::command]
async fn context_detail(state: State<'_, DesktopState>, input: IdInput) -> CommandResult<Value> {
    let id = parse_id(&input.id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let packet = vault
        .get_context_packet(Actor::user(), &id)?
        .ok_or_else(|| CommandError::new("not_found", "The context packet was not found."))?;
    Ok(json!(packet))
}

#[tauri::command]
async fn explore_send(
    state: State<'_, DesktopState>,
    input: ExploreSendInput,
) -> CommandResult<Value> {
    if !(1..=32_768).contains(&input.max_output_tokens) {
        return Err(CommandError::input(
            "Maximum output tokens must be between 1 and 32768.",
        ));
    }
    let context_id = parse_id(&input.context_id)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    let packet = vault
        .get_context_packet(Actor::user(), &context_id)?
        .ok_or_else(|| CommandError::new("not_found", "The context preview was not found."))?;
    if packet.dispatched_at.is_some() {
        return Err(CommandError::input(
            "This context preview was already used; build a fresh preview for every provider call.",
        ));
    }
    let profile = vault
        .get_provider_profile(Actor::user(), &packet.provider_profile_id)?
        .ok_or_else(|| CommandError::new("not_found", "The provider profile was not found."))?;
    let package = MethodPackage::load(&state.paths.methods, &packet.method)?;
    if !package.supports(&profile.capabilities) {
        return Err(CommandError::input(
            "The provider no longer satisfies this method's declared requirements.",
        ));
    }
    let adapter = provider_adapter(profile.clone())?;
    let session = if let Some(identifier) = input.session_id.as_deref() {
        vault
            .get_session(Actor::user(), &parse_id(identifier)?)?
            .ok_or_else(|| CommandError::new("not_found", "The session was not found."))?
    } else {
        require_safe_input(&joined_optional_user_text([input.title.as_deref()]), None)?;
        let session = ReflectionSession::user_started(
            packet.method.clone(),
            nonempty_option(input.title),
            packet.query_sensitivity,
            Utc::now(),
        );
        vault.create_session(Actor::user(), &session)?;
        session
    };
    packet.authorize_session_egress(&session, &profile)?;
    let prior_messages = vault.list_session_messages(Actor::user(), &session.id, 2_000)?;
    let after = prior_messages
        .last()
        .map_or(session.started_at, |message| message.created_at);
    let user_sensitivity = session.sensitivity.max(packet.query_sensitivity);
    let user_message = SessionMessage::user(
        session.id.clone(),
        packet.query.clone(),
        user_sensitivity,
        next_timestamp(after)?,
    );
    vault.append_session_message(Actor::user(), &user_message)?;

    let provenance = Source::model(
        packet.method.clone(),
        profile.name.clone(),
        profile.model.clone(),
        packet.id.clone(),
    );
    let assistant_sensitivity = packet
        .items
        .iter()
        .map(|item| item.sensitivity)
        .fold(user_sensitivity, Sensitivity::max);
    let dispatched =
        vault.mark_context_packet_dispatched(Actor::user(), &context_id, Utc::now())?;
    let response = adapter
        .generate(GenerationRequest {
            context: dispatched,
            instructions: package.provider_instructions(),
            output: OutputContract::Text,
            max_output_tokens: input.max_output_tokens,
        })
        .await?;
    let assistant = SessionMessage::assistant(
        session.id.clone(),
        response.text.clone(),
        assistant_sensitivity,
        provenance,
        next_timestamp(user_message.created_at)?,
    );
    vault.append_session_message(Actor::user(), &assistant)?;
    Ok(json!({
        "sessionId": session.id,
        "userMessageId": user_message.id,
        "assistantMessageId": assistant.id,
        "response": response
    }))
}

#[tauri::command]
async fn data_export(state: State<'_, DesktopState>, input: ExportInput) -> CommandResult<Value> {
    if !input.acknowledge_plaintext {
        return Err(CommandError::input(
            "Portable JSON, JSONL, and Markdown exports are plaintext and require explicit acknowledgement.",
        ));
    }
    let destination = parse_user_path(&input.destination)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    match input.format.as_str() {
        "json" => Ok(json!(vault.export_portable(
            Actor::user(),
            destination,
            ExportFormat::Json,
            Utc::now(),
        )?)),
        "jsonl" => Ok(json!(vault.export_portable(
            Actor::user(),
            destination,
            ExportFormat::Jsonl,
            Utc::now(),
        )?)),
        "markdown" => Ok(json!(vault.export_markdown(
            Actor::user(),
            destination,
            Utc::now(),
        )?)),
        _ => Err(CommandError::input(
            "Export format must be json, jsonl, or markdown.",
        )),
    }
}

#[tauri::command]
async fn data_import(state: State<'_, DesktopState>, input: ImportInput) -> CommandResult<Value> {
    if !input.confirm_empty_vault_import {
        return Err(CommandError::input(
            "Portable import requires explicit confirmation and an empty vault.",
        ));
    }
    let source = parse_user_path(&input.source)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(vault.import_portable(
        Actor::user(),
        source,
        Utc::now(),
    )?))
}

#[tauri::command]
async fn data_backup(state: State<'_, DesktopState>, input: BackupInput) -> CommandResult<Value> {
    let passphrase = Zeroizing::new(input.recovery_passphrase);
    let confirmation = Zeroizing::new(input.recovery_confirmation);
    if passphrase.as_str() != confirmation.as_str() {
        return Err(CommandError::input("The backup passphrases do not match."));
    }
    let destination = parse_user_path(&input.destination)?;
    let unlocked = state.unlocked.lock().await;
    let vault = unlocked.as_ref().ok_or_else(CommandError::locked)?;
    Ok(json!(vault.create_backup(
        Actor::user(),
        destination,
        &passphrase,
        Utc::now(),
    )?))
}

#[tauri::command]
async fn vault_restore_backup(
    state: State<'_, DesktopState>,
    input: RestoreBackupInput,
) -> CommandResult<Value> {
    let source = parse_user_path(&input.source)?;
    let passphrase = Zeroizing::new(input.recovery_passphrase);
    let mut unlocked = state.unlocked.lock().await;
    if unlocked.is_some() || state.paths.vault.exists() || state.paths.recovery.exists() {
        return Err(CommandError::new(
            "already_exists",
            "Restore requires a fresh local target and never overwrites a vault or recovery artifact.",
        ));
    }
    let RestoredVault { vault, key } = SqlCipherVault::restore_backup(
        Actor::user(),
        source,
        &state.paths.vault,
        &passphrase,
        Utc::now(),
    )?;
    let vault_id = match vault.vault_id() {
        Ok(id) => id,
        Err(error) => {
            drop(vault);
            cleanup_vault_files(&state.paths.vault);
            return Err(error.into());
        }
    };
    let envelope = match RecoveryKeyEnvelope::seal(&key, &passphrase) {
        Ok(envelope) => envelope,
        Err(error) => {
            drop(vault);
            cleanup_vault_files(&state.paths.vault);
            return Err(error.into());
        }
    };
    if let Err(error) = write_private_json_new(&state.paths.recovery, &envelope) {
        drop(vault);
        cleanup_vault_files(&state.paths.vault);
        return Err(error);
    }
    let keyring_saved = input.save_to_keyring
        && OsSecretStore
            .put_vault_key(&state.paths.vault, &key)
            .is_ok();
    *unlocked = Some(vault);
    Ok(json!({
        "vaultId": vault_id,
        "vaultPath": state.paths.vault,
        "recoveryPath": state.paths.recovery,
        "keyringSaved": keyring_saved
    }))
}

fn default_paths(methods: PathBuf) -> CommandResult<DesktopPaths> {
    let directories = ProjectDirs::from("org", "Quisum", "Quisum").ok_or_else(|| {
        CommandError::new(
            "platform_path_unavailable",
            "Could not resolve the platform application-data directory.",
        )
    })?;
    let vault = directories.data_local_dir().join("vault.db");
    let mut recovery_name = vault
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| OsString::from("vault.db"));
    recovery_name.push(".recovery.json");
    let recovery = vault.with_file_name(recovery_name);
    Ok(DesktopPaths {
        vault,
        recovery,
        methods,
    })
}

fn resolve_methods_path<R: tauri::Runtime>(app: &tauri::App<R>) -> PathBuf {
    if let Ok(resources) = app.path().resource_dir() {
        let candidate = resources.join("methods");
        if candidate.is_dir() {
            return candidate;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../methods")
}

fn vault_summary(vault: &SqlCipherVault) -> CommandResult<Value> {
    Ok(json!({
        "vaultId": vault.vault_id()?,
        "schemaVersion": vault.schema_version()?,
        "auditRecords": vault.audit_count()?,
        "encrypted": true,
        "telemetry": false
    }))
}

fn require_safe_input(input: &str, region: Option<&str>) -> CommandResult<()> {
    let decision = SafetyEngine.screen_user_input(input);
    match decision.route {
        SafetyRoute::Continue if decision.allow_persistence => Ok(()),
        SafetyRoute::Crisis => match CrisisResourcePack::embedded() {
            Ok(pack) => Err(CommandError::crisis(pack.guidance(region))),
            Err(_) => Err(CommandError::new(
                "crisis_route",
                "Quisum is not an emergency service. Stop identity analysis. If anyone may be in immediate danger, contact local emergency services now and contact a trusted person who can help without putting themselves in danger.",
            )),
        },
        SafetyRoute::RefuseThirdPartyProfiling => Err(CommandError::new(
            "third_party_profiling",
            "Quisum refuses covert profiling of another person. Reflect on your own observations, needs, boundaries, and choices instead.",
        )),
        SafetyRoute::Pause | SafetyRoute::Continue => Err(DomainError::SafetyBlocked.into()),
    }
}

fn joined_user_text<const N: usize>(head: &str, lists: [&[String]; N]) -> String {
    let mut value = head.to_owned();
    for item in lists.into_iter().flatten() {
        value.push('\n');
        value.push_str(item);
    }
    value
}

fn joined_optional_user_text<const N: usize>(values: [Option<&str>; N]) -> String {
    let mut joined = String::new();
    for value in values.into_iter().flatten() {
        append_user_text(&mut joined, Some(value));
    }
    joined
}

fn append_user_text(joined: &mut String, value: Option<&str>) {
    if let Some(value) = value {
        if !joined.is_empty() {
            joined.push('\n');
        }
        joined.push_str(value);
    }
}

fn parse_id(value: &str) -> CommandResult<EntityId> {
    value.parse().map_err(CommandError::from)
}

fn parse_id_set(values: Vec<String>) -> CommandResult<BTreeSet<EntityId>> {
    values.into_iter().map(|value| parse_id(&value)).collect()
}

fn parse_optional_time(value: Option<&str>) -> CommandResult<Option<DateTime<Utc>>> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| CommandError::input("Timestamp must be RFC 3339."))
        })
        .transpose()
}

fn parse_sensitivity(value: &str) -> CommandResult<Sensitivity> {
    match value {
        "normal" => Ok(Sensitivity::Normal),
        "private" => Ok(Sensitivity::Private),
        "highly_sensitive" => Ok(Sensitivity::HighlySensitive),
        "local_only" => Ok(Sensitivity::LocalOnly),
        _ => Err(CommandError::input("Unknown sensitivity class.")),
    }
}

fn parse_claim_kind(value: &str) -> CommandResult<ClaimKind> {
    match value {
        "explicit_fact" => Ok(ClaimKind::ExplicitFact),
        "user_self_assessment" => Ok(ClaimKind::UserSelfAssessment),
        "current_state" => Ok(ClaimKind::CurrentState),
        "value" => Ok(ClaimKind::Value),
        "strength" => Ok(ClaimKind::Strength),
        "need_or_boundary" => Ok(ClaimKind::NeedOrBoundary),
        "pattern_hypothesis" => Ok(ClaimKind::PatternHypothesis),
        _ => Err(CommandError::input("Unknown claim kind.")),
    }
}

fn parse_evidence_grade(value: &str) -> CommandResult<EvidenceGrade> {
    match value {
        "limited" => Ok(EvidenceGrade::Limited),
        "moderate" => Ok(EvidenceGrade::Moderate),
        "repeated" => Ok(EvidenceGrade::Repeated),
        "contested" => Ok(EvidenceGrade::Contested),
        _ => Err(CommandError::input("Unknown evidence grade.")),
    }
}

fn parse_review_decision(value: &str) -> CommandResult<ReviewDecision> {
    match value {
        "confirm" => Ok(ReviewDecision::Confirm),
        "revise_and_confirm" => Ok(ReviewDecision::ReviseAndConfirm),
        "uncertain" => Ok(ReviewDecision::Uncertain),
        "reject" => Ok(ReviewDecision::Reject),
        "convert_to_open_question" => Ok(ReviewDecision::ConvertToOpenQuestion),
        _ => Err(CommandError::input("Unknown review decision.")),
    }
}

fn parse_experiment_risk(value: &str) -> CommandResult<ExperimentRisk> {
    match value {
        "low" => Ok(ExperimentRisk::Low),
        "moderate" => Ok(ExperimentRisk::Moderate),
        _ => Err(CommandError::input("Unknown experiment risk.")),
    }
}

fn parse_provider_kind(value: &str) -> CommandResult<ProviderKind> {
    match value {
        "openai" => Ok(ProviderKind::Openai),
        "anthropic" => Ok(ProviderKind::Anthropic),
        "openai_compatible" => Ok(ProviderKind::OpenaiCompatible),
        "local_http" => Ok(ProviderKind::LocalHttp),
        _ => Err(CommandError::input("Unknown provider protocol.")),
    }
}

fn parse_privacy_kind(value: &str) -> CommandResult<PrivacyKind> {
    match value {
        "cloud" => Ok(PrivacyKind::Cloud),
        "local" => Ok(PrivacyKind::Local),
        _ => Err(CommandError::input("Unknown provider privacy boundary.")),
    }
}

fn parse_user_path(value: &str) -> CommandResult<PathBuf> {
    if value.is_empty()
        || value.chars().count() > MAX_PATH_CHARACTERS
        || value.as_bytes().contains(&0)
    {
        return Err(CommandError::input("Artifact path is empty or too long."));
    }
    Ok(PathBuf::from(value))
}

fn required_option(value: Option<String>, message: &str) -> CommandResult<String> {
    nonempty_option(value).ok_or_else(|| CommandError::input(message))
}

fn nonempty_option(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn next_timestamp(previous: DateTime<Utc>) -> CommandResult<DateTime<Utc>> {
    let now = Utc::now();
    if now > previous {
        Ok(now)
    } else {
        previous
            .checked_add_signed(chrono::Duration::milliseconds(1))
            .ok_or_else(|| CommandError::input("Local timestamp overflow."))
    }
}

fn provider_adapter(profile: ProviderProfile) -> CommandResult<HttpProvider> {
    let credential = profile
        .credential_ref
        .as_deref()
        .map(|reference| {
            let bytes = OsSecretStore.get_provider_secret(reference)?;
            ProviderCredential::new(bytes.to_vec()).map_err(CommandError::from)
        })
        .transpose()?;
    HttpProvider::new(profile, credential).map_err(CommandError::from)
}

fn read_recovery_envelope(path: &Path) -> CommandResult<RecoveryKeyEnvelope> {
    let metadata = fs::symlink_metadata(path).map_err(|_| {
        CommandError::new(
            "recovery_missing",
            "The local recovery envelope was not found.",
        )
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_RECOVERY_FILE_BYTES
    {
        return Err(CommandError::new(
            "invalid_recovery",
            "The recovery envelope is invalid or exceeds the safe size limit.",
        ));
    }
    let mut file = fs::File::open(path).map_err(|_| {
        CommandError::new(
            "recovery_read_failed",
            "The recovery envelope could not be read.",
        )
    })?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    std::io::Read::by_ref(&mut file)
        .take(MAX_RECOVERY_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            CommandError::new(
                "recovery_read_failed",
                "The recovery envelope could not be read.",
            )
        })?;
    if bytes.len() as u64 > MAX_RECOVERY_FILE_BYTES {
        return Err(CommandError::new(
            "invalid_recovery",
            "The recovery envelope exceeds the safe size limit.",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        CommandError::new(
            "invalid_recovery",
            "The recovery envelope is not valid JSON.",
        )
    })
}

fn write_private_json_new<T: Serialize>(path: &Path, value: &T) -> CommandResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|_| {
            CommandError::new(
                "local_operation_failed",
                "The private application-data directory could not be created.",
            )
        })?;
    }
    let options = &mut OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut created = false;
    let result = (|| -> CommandResult<()> {
        let mut file = options.open(path).map_err(|_| {
            CommandError::new(
                "already_exists",
                "Quisum never overwrites an existing recovery artifact.",
            )
        })?;
        created = true;
        serde_json::to_writer(&mut file, value).map_err(|_| {
            CommandError::new(
                "local_operation_failed",
                "Recovery material could not be encoded.",
            )
        })?;
        file.write_all(b"\n").map_err(|_| {
            CommandError::new(
                "local_operation_failed",
                "Recovery material could not be written.",
            )
        })?;
        file.sync_all().map_err(|_| {
            CommandError::new(
                "local_operation_failed",
                "Recovery material could not be synchronized.",
            )
        })?;
        Ok(())
    })();
    if result.is_err() && created {
        remove_file_if_present(path);
    }
    result
}

fn cleanup_vault_files(path: &Path) {
    remove_file_if_present(path);
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = OsString::from(path.as_os_str());
        sidecar.push(suffix);
        remove_file_if_present(&PathBuf::from(sidecar));
    }
}

fn remove_file_if_present(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

fn ignore_secret_delete(reference: &str) {
    match OsSecretStore.delete_provider_secret(reference) {
        Ok(()) | Err(VaultError::SecretNotFound) => {}
        Err(_) => {}
    }
}

/// Start the packaged desktop shell.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .setup(|app| {
            let methods = resolve_methods_path(app);
            let paths = default_paths(methods).map_err(|error| {
                std::io::Error::other(format!("{}: {}", error.code, error.message))
            })?;
            app.manage(DesktopState::new(paths));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            vault_create,
            vault_unlock_keyring,
            vault_unlock_recovery,
            vault_lock,
            vault_doctor,
            vault_restore_backup,
            method_list,
            event_list,
            event_capture,
            event_review,
            event_delete,
            claim_list,
            claim_review_queue,
            claim_create,
            claim_review,
            claim_counterevidence,
            question_list,
            question_review,
            experiment_list,
            experiment_create,
            experiment_transition,
            session_list,
            session_detail,
            session_transition,
            memory_search,
            reflog_list,
            provider_list,
            provider_create,
            provider_toggle,
            provider_revoke,
            provider_test,
            context_preview,
            context_approve,
            context_detail,
            explore_send,
            data_export,
            data_import,
            data_backup
        ])
        .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn command_errors_do_not_expose_filesystem_paths_or_sources() {
        let error = CommandError::from(VaultError::FileSystem {
            path: PathBuf::from("/synthetic/private/path"),
            source: std::io::Error::other("synthetic source details"),
        });
        let encoded = serde_json::to_string(&error).unwrap_or_default();
        assert!(!encoded.contains("private/path"));
        assert!(!encoded.contains("source details"));
    }

    #[test]
    fn recovery_artifacts_are_exclusive_and_owner_only() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let path = directory.path().join("recovery.json");
        let key = VaultKey::generate()?;
        let envelope = RecoveryKeyEnvelope::seal(&key, "synthetic passphrase")?;
        write_private_json_new(&path, &envelope).map_err(|error| error.message)?;
        let before = fs::read(&path)?;
        assert!(write_private_json_new(&path, &envelope).is_err());
        assert_eq!(fs::read(&path)?, before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    }

    #[test]
    fn path_and_enum_inputs_are_strict() {
        assert!(parse_user_path("").is_err());
        assert!(parse_user_path(&"x".repeat(MAX_PATH_CHARACTERS + 1)).is_err());
        assert!(parse_sensitivity("secret").is_err());
        assert!(parse_provider_kind("custom").is_err());
    }

    #[test]
    fn crisis_errors_include_versioned_guidance_without_repeating_input()
    -> Result<(), Box<dyn Error>> {
        let input = "I have the means beside me and plan to hurt myself tonight.";
        let error = match require_safe_input(input, Some("en-US")) {
            Ok(()) => return Err("crisis input was not stopped".into()),
            Err(error) => error,
        };
        let encoded = serde_json::to_value(error)?;
        assert_eq!(encoded["code"], "crisis_route");
        assert_eq!(encoded["crisisGuidance"]["packVersion"], "1.0.0");
        assert!(matches!(
            encoded["crisisGuidance"]["region"].as_str(),
            Some("US" | "DEFAULT")
        ));
        assert!(!encoded.to_string().contains(input));
        Ok(())
    }
}
