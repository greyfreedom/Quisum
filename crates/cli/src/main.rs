#![doc = "Secure command-line entry point for Quisum."]
#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, BufRead as _, IsTerminal as _, Read as _, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use clap::{Args, Parser, Subcommand, ValueEnum};
use directories::ProjectDirs;
use futures_util::StreamExt as _;
use quisum_core::{
    Actor, BehavioralExperiment, Claim, ClaimKind, ContextBuilder, CrisisResourcePack, DomainError,
    EntityId, Event, EvidenceGrade, ExperimentRisk, MethodError, MethodPackage,
    OFFICIAL_METHOD_IDS, OpenQuestionStatus, PrivacyKind, ProviderCapabilities, ProviderKind,
    ProviderProfile, ReflectionSession, ReviewDecision, ReviewRequest, SafetyEngine, SafetyRoute,
    Sensitivity, SessionMessage, Source, Validate,
};
use quisum_providers::{
    GenerationRequest, HttpProvider, ModelProvider as _, OutputContract, ProviderCredential,
    ProviderError, ProviderEvent,
};
use quisum_vault::{
    ExportFormat, OsSecretStore, RecoveryKeyEnvelope, RestoredVault, SqlCipherVault, VaultError,
    VaultKey,
};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;
use zeroize::Zeroizing;

const MAX_SECRET_BYTES: usize = 16 * 1_024;
const MAX_RECOVERY_FILE_BYTES: u64 = 64 * 1_024;

#[derive(Debug, Parser)]
#[command(
    name = "quisum",
    version,
    about = "Local-first, evidence-governed self-inquiry",
    long_about = None
)]
struct Cli {
    /// Encrypted SQLCipher vault path.
    #[arg(long, global = true, value_name = "PATH")]
    vault: Option<PathBuf>,

    /// Recovery-key envelope path.
    #[arg(long, global = true, value_name = "PATH")]
    recovery: Option<PathBuf>,

    /// Root containing declarative method packages.
    #[arg(long, global = true, value_name = "PATH")]
    methods: Option<PathBuf>,

    /// Optional ISO region or locale used only for local crisis-resource selection.
    #[arg(long, global = true)]
    region: Option<String>,

    /// Emit compact JSON (streaming chat emits NDJSON events).
    #[arg(long, global = true)]
    json: bool,

    /// Do not read or write the operating-system credential store.
    #[arg(long, global = true)]
    no_keyring: bool,

    /// Read a recovery passphrase from one line of stdin instead of a terminal prompt.
    #[arg(long, global = true)]
    unlock_passphrase_stdin: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new encrypted vault and recovery envelope.
    Init(InitArgs),
    /// Show content-free vault status.
    Status,
    /// Verify the cipher, schema, and database integrity.
    Doctor,
    /// Capture, inspect, review, or delete concrete events.
    Event {
        #[command(subcommand)]
        command: EventCommand,
    },
    /// Propose, inspect, and human-review evidence-backed claims.
    Claim {
        #[command(subcommand)]
        command: ClaimCommand,
    },
    /// Inspect and human-review unresolved questions without turning them into claims.
    Question {
        #[command(subcommand)]
        command: QuestionCommand,
    },
    /// Start, inspect, close, or delete reflection sessions.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Search locally eligible memory.
    Search(SearchArgs),
    /// Build, inspect, or explicitly approve provider context.
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },
    /// Configure and use model providers.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Discover or validate the installed declarative method packages.
    Method {
        #[command(subcommand)]
        command: MethodCommand,
    },
    /// Track voluntary behavioral experiments.
    Experiment {
        #[command(subcommand)]
        command: ExperimentCommand,
    },
    /// Show content-free append-only activity history.
    Reflog(LimitArgs),
    /// Export a portable plaintext artifact or derived Markdown.
    Export(ExportArgs),
    /// Import a checked portable artifact into an empty vault.
    Import(ImportArgs),
    /// Create a passphrase-recoverable encrypted backup.
    Backup(BackupArgs),
    /// Restore an encrypted backup to a new vault path.
    Restore(RestoreArgs),
    /// Run the least-privilege local MCP server.
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Permit placing encrypted vault artifacts inside a Git work tree.
    #[arg(long)]
    allow_git_directory: bool,

    /// Read two recovery-passphrase lines from stdin.
    #[arg(long)]
    recovery_passphrase_stdin: bool,
}

#[derive(Debug, Subcommand)]
enum EventCommand {
    Add(Box<EventAddArgs>),
    List(LimitArgs),
    Show(IdArgs),
    Review(EventReviewArgs),
    Delete(DeleteArgs),
}

#[derive(Debug, Args)]
struct EventAddArgs {
    #[arg(long)]
    title: String,
    /// Observable or directly reported fact; repeat for multiple facts.
    #[arg(long = "fact", required = true)]
    facts: Vec<String>,
    #[arg(long = "interpretation")]
    interpretations: Vec<String>,
    #[arg(long = "emotion")]
    emotions: Vec<String>,
    #[arg(long = "body")]
    bodily_responses: Vec<String>,
    #[arg(long = "action")]
    actions: Vec<String>,
    #[arg(long = "short-outcome")]
    short_term_outcomes: Vec<String>,
    #[arg(long = "long-outcome")]
    long_term_outcomes: Vec<String>,
    #[arg(long = "topic")]
    topics: Vec<String>,
    #[arg(long, value_enum, default_value = "private")]
    sensitivity: SensitivityArg,
    /// RFC 3339 occurrence time.
    #[arg(long)]
    occurred_at: Option<String>,
}

#[derive(Debug, Args)]
struct EventReviewArgs {
    id: String,
    #[arg(long, value_enum)]
    decision: EventDecisionArg,
}

#[derive(Debug, Args)]
struct DeleteArgs {
    id: String,
    /// Confirm irreversible deletion and dependent-claim repair.
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
enum ClaimCommand {
    Propose(ClaimProposeArgs),
    List(ClaimListArgs),
    Show(IdArgs),
    Review(ClaimReviewArgs),
    ProposeCounterevidence(CounterevidenceArgs),
    AcceptCounterevidence(CounterevidenceArgs),
}

#[derive(Debug, Subcommand)]
enum QuestionCommand {
    List(LimitArgs),
    Show(IdArgs),
    Review(QuestionReviewArgs),
}

#[derive(Debug, Args)]
struct QuestionReviewArgs {
    id: String,
    #[arg(long, value_enum)]
    decision: QuestionDecisionArg,
}

#[derive(Debug, Args)]
struct ClaimProposeArgs {
    #[arg(long, value_enum)]
    kind: ClaimKindArg,
    #[arg(long)]
    statement: String,
    #[arg(long = "context")]
    contexts: Vec<String>,
    #[arg(long = "support")]
    supporting_evidence: Vec<String>,
    #[arg(long = "counter")]
    counterevidence: Vec<String>,
    #[arg(long = "alternative")]
    alternatives: Vec<String>,
    #[arg(long, value_enum, default_value = "limited")]
    evidence_grade: EvidenceGradeArg,
    #[arg(long, value_enum, default_value = "private")]
    sensitivity: SensitivityArg,
    /// Required RFC 3339 revisit time for `current_state` claims.
    #[arg(long)]
    revisit_after: Option<String>,
}

#[derive(Debug, Args)]
struct ClaimListArgs {
    #[arg(long, default_value_t = 100)]
    limit: usize,
    /// Show only claims needing a human decision.
    #[arg(long)]
    review_queue: bool,
}

#[derive(Debug, Args)]
struct ClaimReviewArgs {
    id: String,
    #[arg(long, value_enum)]
    decision: ReviewDecisionArg,
    #[arg(long)]
    edited_statement: Option<String>,
    #[arg(long)]
    note: Option<String>,
}

#[derive(Debug, Args)]
struct CounterevidenceArgs {
    #[arg(long)]
    claim: String,
    #[arg(long)]
    evidence: String,
}

#[derive(Debug, Args)]
struct SearchArgs {
    query: String,
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Debug, Subcommand)]
enum ContextCommand {
    Build(ContextBuildArgs),
    Show(IdArgs),
    Approve(IdArgs),
}

#[derive(Debug, Args)]
struct ContextBuildArgs {
    #[arg(long)]
    provider: String,
    #[arg(long)]
    purpose: String,
    #[arg(long)]
    method: String,
    #[arg(long)]
    query: String,
    #[arg(long, value_enum, default_value = "private")]
    query_sensitivity: SensitivityArg,
    #[arg(long, default_value_t = 12)]
    maximum_items: usize,
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    Add(ProviderAddArgs),
    List,
    Test(IdArgs),
    /// Allow new calls through an existing profile.
    Enable(IdArgs),
    /// Block new calls while preserving historical provenance.
    Disable(IdArgs),
    /// Disable a profile and delete its API credential from the OS credential store.
    RevokeSecret(IdArgs),
    Chat(ChatArgs),
}

#[derive(Debug, Subcommand)]
enum MethodCommand {
    /// List official V1 packages after validating each package.
    List,
    /// Validate one canonical method identifier, or all official methods when omitted.
    Validate(MethodValidateArgs),
}

#[derive(Debug, Args)]
struct MethodValidateArgs {
    method: Option<String>,
}

#[derive(Debug, Args)]
struct ProviderAddArgs {
    #[arg(long)]
    name: String,
    #[arg(long, value_enum)]
    kind: ProviderKindArg,
    #[arg(long)]
    base_url: String,
    #[arg(long)]
    model: String,
    #[arg(long, value_enum)]
    privacy: PrivacyKindArg,
    /// Permitted outbound sensitivity; repeat as needed. `local_only` is forbidden.
    #[arg(long = "trust", value_enum, required = true)]
    trusted_sensitivities: Vec<SensitivityArg>,
    #[arg(long)]
    streaming: bool,
    #[arg(long)]
    json_schema: bool,
    #[arg(long)]
    tool_use: bool,
    #[arg(long)]
    vision: bool,
    #[arg(long)]
    context_limit: Option<u32>,
    #[arg(long)]
    retention_note: Option<String>,
    /// Prompt for an API credential even when the endpoint is local.
    #[arg(long)]
    with_secret: bool,
    /// Read the API credential from stdin. The credential itself is never a CLI argument.
    #[arg(long)]
    secret_stdin: bool,
}

#[derive(Debug, Args)]
struct ChatArgs {
    /// A previously previewed context packet. High-sensitivity packets must already be approved.
    #[arg(long)]
    context: String,
    /// Existing active session. If omitted, Quisum starts one from the packet method.
    #[arg(long)]
    session: Option<String>,
    /// Optional title when a new session is created.
    #[arg(long)]
    title: Option<String>,
    #[arg(long, default_value_t = 2_048, value_parser = clap::value_parser!(u32).range(1..=32768))]
    max_output_tokens: u32,
    #[arg(long)]
    stream: bool,
}

#[derive(Debug, Subcommand)]
enum ExperimentCommand {
    Propose(ExperimentProposeArgs),
    List(LimitArgs),
    Show(IdArgs),
    Activate(IdArgs),
    Complete(ExperimentCompleteArgs),
    Cancel(IdArgs),
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Start(SessionStartArgs),
    List(LimitArgs),
    Show(IdArgs),
    Complete(IdArgs),
    Abandon(IdArgs),
    Delete(DeleteArgs),
}

#[derive(Debug, Args)]
struct SessionStartArgs {
    #[arg(long)]
    method: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long, value_enum, default_value = "private")]
    sensitivity: SensitivityArg,
}

#[derive(Debug, Args)]
struct ExperimentProposeArgs {
    #[arg(long)]
    title: String,
    #[arg(long)]
    hypothesis: String,
    #[arg(long)]
    prediction: String,
    #[arg(long)]
    action: String,
    #[arg(long)]
    stop_condition: String,
    #[arg(long, value_enum, default_value = "low")]
    risk: ExperimentRiskArg,
    #[arg(long = "claim")]
    related_claims: Vec<String>,
    #[arg(long)]
    expected_result: Option<String>,
    #[arg(long, value_enum, default_value = "private")]
    sensitivity: SensitivityArg,
}

#[derive(Debug, Args)]
struct ExperimentCompleteArgs {
    id: String,
    #[arg(long)]
    actual_result: String,
    #[arg(long)]
    learning: String,
}

#[derive(Debug, Args)]
struct ExportArgs {
    destination: PathBuf,
    #[arg(long, value_enum, default_value = "jsonl")]
    format: PortableFormatArg,
    /// Acknowledge that JSON, JSONL, and Markdown exports are plaintext.
    #[arg(long)]
    acknowledge_plaintext: bool,
}

#[derive(Debug, Args)]
struct ImportArgs {
    source: PathBuf,
}

#[derive(Debug, Args)]
struct BackupArgs {
    destination: PathBuf,
    /// Read two new backup-passphrase lines from stdin.
    #[arg(long)]
    recovery_passphrase_stdin: bool,
}

#[derive(Debug, Args)]
struct RestoreArgs {
    source: PathBuf,
    destination: PathBuf,
    /// Read the backup passphrase from stdin.
    #[arg(long)]
    recovery_passphrase_stdin: bool,
    /// Permit placing the restored encrypted vault inside a Git work tree.
    #[arg(long)]
    allow_git_directory: bool,
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    /// Serve stdio MCP. Unlock must use the OS credential store because stdin carries protocol data.
    Serve,
}

#[derive(Debug, Args)]
struct IdArgs {
    id: String,
}

#[derive(Debug, Args)]
struct LimitArgs {
    #[arg(long, default_value_t = 100)]
    limit: usize,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum SensitivityArg {
    Normal,
    Private,
    HighlySensitive,
    LocalOnly,
}

impl From<SensitivityArg> for Sensitivity {
    fn from(value: SensitivityArg) -> Self {
        match value {
            SensitivityArg::Normal => Self::Normal,
            SensitivityArg::Private => Self::Private,
            SensitivityArg::HighlySensitive => Self::HighlySensitive,
            SensitivityArg::LocalOnly => Self::LocalOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum EventDecisionArg {
    Accept,
    Reject,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum ClaimKindArg {
    ExplicitFact,
    UserSelfAssessment,
    CurrentState,
    Value,
    Strength,
    NeedOrBoundary,
    PatternHypothesis,
}

impl From<ClaimKindArg> for ClaimKind {
    fn from(value: ClaimKindArg) -> Self {
        match value {
            ClaimKindArg::ExplicitFact => Self::ExplicitFact,
            ClaimKindArg::UserSelfAssessment => Self::UserSelfAssessment,
            ClaimKindArg::CurrentState => Self::CurrentState,
            ClaimKindArg::Value => Self::Value,
            ClaimKindArg::Strength => Self::Strength,
            ClaimKindArg::NeedOrBoundary => Self::NeedOrBoundary,
            ClaimKindArg::PatternHypothesis => Self::PatternHypothesis,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum EvidenceGradeArg {
    Limited,
    Moderate,
    Repeated,
    Contested,
}

impl From<EvidenceGradeArg> for EvidenceGrade {
    fn from(value: EvidenceGradeArg) -> Self {
        match value {
            EvidenceGradeArg::Limited => Self::Limited,
            EvidenceGradeArg::Moderate => Self::Moderate,
            EvidenceGradeArg::Repeated => Self::Repeated,
            EvidenceGradeArg::Contested => Self::Contested,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum ReviewDecisionArg {
    Confirm,
    ReviseAndConfirm,
    Uncertain,
    Reject,
    ConvertToOpenQuestion,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum QuestionDecisionArg {
    /// Accept a proposed question into the open-question list.
    Accept,
    /// Reject a proposed question without treating it as answered.
    Reject,
    /// Mark an accepted open question as answered by later evidence.
    Answered,
    /// Close an accepted open question without an answer.
    Close,
}

impl From<QuestionDecisionArg> for OpenQuestionStatus {
    fn from(value: QuestionDecisionArg) -> Self {
        match value {
            QuestionDecisionArg::Accept => Self::Open,
            QuestionDecisionArg::Reject | QuestionDecisionArg::Close => Self::Closed,
            QuestionDecisionArg::Answered => Self::Answered,
        }
    }
}

impl From<ReviewDecisionArg> for ReviewDecision {
    fn from(value: ReviewDecisionArg) -> Self {
        match value {
            ReviewDecisionArg::Confirm => Self::Confirm,
            ReviewDecisionArg::ReviseAndConfirm => Self::ReviseAndConfirm,
            ReviewDecisionArg::Uncertain => Self::Uncertain,
            ReviewDecisionArg::Reject => Self::Reject,
            ReviewDecisionArg::ConvertToOpenQuestion => Self::ConvertToOpenQuestion,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum ProviderKindArg {
    Openai,
    Anthropic,
    OpenaiCompatible,
    LocalHttp,
}

impl From<ProviderKindArg> for ProviderKind {
    fn from(value: ProviderKindArg) -> Self {
        match value {
            ProviderKindArg::Openai => Self::Openai,
            ProviderKindArg::Anthropic => Self::Anthropic,
            ProviderKindArg::OpenaiCompatible => Self::OpenaiCompatible,
            ProviderKindArg::LocalHttp => Self::LocalHttp,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum PrivacyKindArg {
    Cloud,
    Local,
}

impl From<PrivacyKindArg> for PrivacyKind {
    fn from(value: PrivacyKindArg) -> Self {
        match value {
            PrivacyKindArg::Cloud => Self::Cloud,
            PrivacyKindArg::Local => Self::Local,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum ExperimentRiskArg {
    Low,
    Moderate,
}

impl From<ExperimentRiskArg> for ExperimentRisk {
    fn from(value: ExperimentRiskArg) -> Self {
        match value {
            ExperimentRiskArg::Low => Self::Low,
            ExperimentRiskArg::Moderate => Self::Moderate,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "snake_case")]
enum PortableFormatArg {
    Json,
    Jsonl,
    Markdown,
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Method(#[from] MethodError),
    #[error("local JSON processing failed")]
    Json(#[from] serde_json::Error),
    #[error("local filesystem or terminal operation failed")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Safety(String),
}

#[derive(Debug)]
struct RuntimeConfig {
    vault: PathBuf,
    recovery: PathBuf,
    methods: PathBuf,
    json: bool,
    no_keyring: bool,
    unlock_passphrase_stdin: bool,
    region: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("quisum: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(cli: Cli) -> Result<(), CliError> {
    let vault = cli.vault.unwrap_or(default_vault_path()?);
    let recovery = cli
        .recovery
        .unwrap_or_else(|| default_recovery_path(&vault));
    let methods = cli.methods.unwrap_or_else(default_methods_path);
    let config = RuntimeConfig {
        vault,
        recovery,
        methods,
        json: cli.json,
        no_keyring: cli.no_keyring,
        unlock_passphrase_stdin: cli.unlock_passphrase_stdin,
        region: cli.region,
    };

    match cli.command {
        Command::Init(args) => init(&config, args),
        Command::Restore(args) => restore(&config, args),
        Command::Mcp {
            command: McpCommand::Serve,
        } => serve_mcp(&config).await,
        Command::Method { command } => method_command(&config, command),
        command => {
            let vault = unlock_vault(&config)?;
            match command {
                Command::Status => status(&config, &vault),
                Command::Doctor => doctor(&config, &vault),
                Command::Event { command } => event_command(&config, &vault, command),
                Command::Claim { command } => claim_command(&config, &vault, command),
                Command::Question { command } => question_command(&config, &vault, command),
                Command::Session { command } => session_command(&config, &vault, command),
                Command::Search(args) => search(&config, &vault, args),
                Command::Context { command } => context_command(&config, &vault, command),
                Command::Provider { command } => provider_command(&config, &vault, command).await,
                Command::Experiment { command } => experiment_command(&config, &vault, command),
                Command::Reflog(args) => reflog(&config, &vault, args),
                Command::Export(args) => export(&config, &vault, args),
                Command::Import(args) => import(&config, &vault, args),
                Command::Backup(args) => backup(&config, &vault, args),
                Command::Init(_)
                | Command::Restore(_)
                | Command::Method { .. }
                | Command::Mcp { .. } => Err(CliError::Invalid("invalid command dispatch")),
            }
        }
    }
}

fn init(config: &RuntimeConfig, args: InitArgs) -> Result<(), CliError> {
    require_safe_vault_location(&config.vault, args.allow_git_directory)?;
    if config.vault.exists() || config.recovery.exists() {
        return Err(CliError::Invalid(
            "vault or recovery path already exists; Quisum never overwrites either artifact",
        ));
    }
    let passphrase = read_new_passphrase(
        args.recovery_passphrase_stdin,
        "New recovery passphrase: ",
        "Repeat recovery passphrase: ",
    )?;
    let key = VaultKey::generate()?;
    let envelope = RecoveryKeyEnvelope::seal(&key, &passphrase)?;
    write_private_json_new(&config.recovery, &envelope)?;
    let vault = match SqlCipherVault::create(&config.vault, &key) {
        Ok(vault) => vault,
        Err(error) => {
            let _cleanup = fs::remove_file(&config.recovery);
            return Err(error.into());
        }
    };
    let vault_id = vault.vault_id()?;
    let keyring_saved = if config.no_keyring {
        false
    } else {
        OsSecretStore.put_vault_key(&config.vault, &key).is_ok()
    };
    print_value(
        config,
        &json!({
            "vault_id": vault_id,
            "vault_path": config.vault,
            "recovery_path": config.recovery,
            "keyring_saved": keyring_saved,
            "warning": (!keyring_saved).then_some("Keep the recovery file and passphrase safe; OS credential storage was not used.")
        }),
    )
}

fn status(config: &RuntimeConfig, vault: &SqlCipherVault) -> Result<(), CliError> {
    print_value(
        config,
        &json!({
            "product": "Quisum",
            "version": env!("CARGO_PKG_VERSION"),
            "vault_id": vault.vault_id()?,
            "schema_version": vault.schema_version()?,
            "audit_records": vault.audit_count()?,
            "vault_path": config.vault,
            "encrypted": true,
            "telemetry": false
        }),
    )
}

fn doctor(config: &RuntimeConfig, vault: &SqlCipherVault) -> Result<(), CliError> {
    vault.integrity_check()?;
    print_value(
        config,
        &json!({
            "ok": true,
            "sqlcipher": "verified",
            "integrity": "ok",
            "schema_version": vault.schema_version()?,
            "latest_supported_schema": quisum_vault::LATEST_SCHEMA_VERSION,
            "recovery_envelope_present": config.recovery.is_file(),
            "telemetry": false
        }),
    )
}

fn event_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: EventCommand,
) -> Result<(), CliError> {
    match command {
        EventCommand::Add(args) => {
            let safety_text = joined_user_text(
                &args.title,
                [
                    &args.facts,
                    &args.interpretations,
                    &args.emotions,
                    &args.bodily_responses,
                    &args.actions,
                    &args.short_term_outcomes,
                    &args.long_term_outcomes,
                    &args.topics,
                ],
            );
            require_safe_input(&safety_text, config.region.as_deref())?;
            let now = Utc::now();
            let mut event = Event::user_confirmed(
                args.title,
                args.facts,
                args.occurred_at.as_deref().map(parse_time).transpose()?,
                now,
                args.sensitivity.into(),
            );
            event.interpretations = args.interpretations;
            event.emotions = args.emotions;
            event.bodily_responses = args.bodily_responses;
            event.actions = args.actions;
            event.short_term_outcomes = args.short_term_outcomes;
            event.long_term_outcomes = args.long_term_outcomes;
            event.topics = args.topics.into_iter().collect();
            event.validate()?;
            vault.insert_event(Actor::user(), &event)?;
            print_value(config, &event)
        }
        EventCommand::List(args) => {
            print_value(config, &vault.list_events(Actor::user(), args.limit)?)
        }
        EventCommand::Show(args) => {
            let event = vault
                .get_event(Actor::user(), &parse_id(&args.id)?)?
                .ok_or(VaultError::NotFoundRecord)?;
            print_value(config, &event)
        }
        EventCommand::Review(args) => {
            let event = vault.review_event(
                Actor::user(),
                &parse_id(&args.id)?,
                matches!(args.decision, EventDecisionArg::Accept),
                Utc::now(),
            )?;
            print_value(config, &event)
        }
        EventCommand::Delete(args) => {
            if !args.yes {
                return Err(CliError::Invalid(
                    "event deletion requires --yes because dependent claims may be repaired or purged",
                ));
            }
            let impact = vault.delete_event(Actor::user(), &parse_id(&args.id)?, Utc::now())?;
            print_value(
                config,
                &json!({
                    "deleted_event": impact.event_id,
                    "affected_claims": impact.affected_claims,
                    "purged_pending_claims": impact.purged_pending_claims
                }),
            )
        }
    }
}

fn claim_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: ClaimCommand,
) -> Result<(), CliError> {
    match command {
        ClaimCommand::Propose(args) => {
            require_safe_input(
                &joined_user_text(&args.statement, [&args.contexts, &args.alternatives]),
                config.region.as_deref(),
            )?;
            let now = Utc::now();
            let mut claim = Claim::proposal(
                args.kind.into(),
                args.statement,
                args.evidence_grade.into(),
                args.sensitivity.into(),
                Source::user(),
                now,
            );
            claim.contexts = args.contexts.into_iter().collect();
            claim.supporting_evidence = parse_id_set(args.supporting_evidence)?;
            claim.counterevidence = parse_id_set(args.counterevidence)?;
            claim.alternative_explanations = args.alternatives;
            claim.revisit_after = args.revisit_after.as_deref().map(parse_time).transpose()?;
            claim.validate()?;
            vault.insert_claim(Actor::user(), &claim)?;
            print_value(config, &claim)
        }
        ClaimCommand::List(args) => {
            let claims = if args.review_queue {
                vault.list_review_claims(Actor::user(), args.limit.min(100))?
            } else {
                vault.list_claims(Actor::user(), args.limit)?
            };
            print_value(config, &claims)
        }
        ClaimCommand::Show(args) => {
            let id = parse_id(&args.id)?;
            let claim = vault
                .get_claim(Actor::user(), &id)?
                .ok_or(VaultError::NotFoundRecord)?;
            let revisions = vault.list_claim_revisions(Actor::user(), Some(&id), 500)?;
            print_value(config, &json!({"claim": claim, "revisions": revisions}))
        }
        ClaimCommand::Review(args) => {
            require_safe_input(
                &joined_optional_user_text([
                    args.edited_statement.as_deref(),
                    args.note.as_deref(),
                ]),
                config.region.as_deref(),
            )?;
            let commit = vault.review_claim(
                Actor::user(),
                &parse_id(&args.id)?,
                ReviewRequest {
                    decision: args.decision.into(),
                    edited_statement: args.edited_statement,
                    note: args.note,
                },
                Utc::now(),
            )?;
            print_value(
                config,
                &json!({
                    "review": commit.outcome.review,
                    "revision": commit.outcome.revision,
                    "open_question": commit.open_question
                }),
            )
        }
        ClaimCommand::ProposeCounterevidence(args) => {
            let proposal = vault.propose_counterevidence(
                Actor::user(),
                &parse_id(&args.claim)?,
                &parse_id(&args.evidence)?,
                None,
                Utc::now(),
            )?;
            print_value(
                config,
                &json!({
                    "claim_id": proposal.claim_id,
                    "evidence_id": proposal.evidence_id,
                    "status": "proposed",
                    "created_at": proposal.created_at
                }),
            )
        }
        ClaimCommand::AcceptCounterevidence(args) => {
            let claim = vault.accept_counterevidence(
                Actor::user(),
                &parse_id(&args.claim)?,
                &parse_id(&args.evidence)?,
                Utc::now(),
            )?;
            print_value(config, &claim)
        }
    }
}

fn question_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: QuestionCommand,
) -> Result<(), CliError> {
    match command {
        QuestionCommand::List(args) => print_value(
            config,
            &vault.list_open_questions(Actor::user(), args.limit)?,
        ),
        QuestionCommand::Show(args) => {
            let question = vault
                .get_open_question(Actor::user(), &parse_id(&args.id)?)?
                .ok_or(VaultError::NotFoundRecord)?;
            print_value(config, &question)
        }
        QuestionCommand::Review(args) => {
            let question = vault.review_open_question(
                Actor::user(),
                &parse_id(&args.id)?,
                args.decision.into(),
                Utc::now(),
            )?;
            print_value(config, &question)
        }
    }
}

fn session_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: SessionCommand,
) -> Result<(), CliError> {
    match command {
        SessionCommand::Start(args) => {
            require_safe_input(
                &joined_optional_user_text([args.title.as_deref()]),
                config.region.as_deref(),
            )?;
            let package = MethodPackage::load(&config.methods, &args.method)?;
            let session = ReflectionSession::user_started(
                package.identifier(),
                args.title,
                args.sensitivity.into(),
                Utc::now(),
            );
            session.validate()?;
            vault.create_session(Actor::user(), &session)?;
            print_value(
                config,
                &json!({
                    "session": session,
                    "method_review_status": package.manifest.review.status
                }),
            )
        }
        SessionCommand::List(args) => {
            print_value(config, &vault.list_sessions(Actor::user(), args.limit)?)
        }
        SessionCommand::Show(args) => {
            let id = parse_id(&args.id)?;
            let session = vault
                .get_session(Actor::user(), &id)?
                .ok_or(VaultError::NotFoundRecord)?;
            let messages = vault.list_session_messages(Actor::user(), &id, 2_000)?;
            print_value(config, &json!({"session": session, "messages": messages}))
        }
        SessionCommand::Complete(args) => {
            let session =
                vault.close_session(Actor::user(), &parse_id(&args.id)?, true, Utc::now())?;
            print_value(config, &session)
        }
        SessionCommand::Abandon(args) => {
            let session =
                vault.close_session(Actor::user(), &parse_id(&args.id)?, false, Utc::now())?;
            print_value(config, &session)
        }
        SessionCommand::Delete(args) => {
            if !args.yes {
                return Err(CliError::Invalid(
                    "session deletion requires --yes because the transcript is irreversibly removed",
                ));
            }
            let session_id = parse_id(&args.id)?;
            let impact = vault.delete_session(Actor::user(), &session_id, Utc::now())?;
            print_value(
                config,
                &json!({
                    "deleted_session": impact.session_id,
                    "deleted_messages": impact.deleted_messages,
                    "affected_claims": impact.affected_claims,
                    "purged_pending_claims": impact.purged_pending_claims
                }),
            )
        }
    }
}

fn search(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    args: SearchArgs,
) -> Result<(), CliError> {
    let results = vault.search_memory(Actor::user(), &args.query, args.limit)?;
    let values = results
        .into_iter()
        .map(|candidate| {
            json!({
                "item_type": candidate.item_type,
                "item_id": candidate.item_id,
                "excerpt": candidate.excerpt,
                "sensitivity": candidate.sensitivity,
                "selection_reason": candidate.selection_reason,
                "relevance_score": candidate.relevance_score,
                "recorded_at": candidate.recorded_at
            })
        })
        .collect::<Vec<_>>();
    print_value(config, &values)
}

fn context_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: ContextCommand,
) -> Result<(), CliError> {
    match command {
        ContextCommand::Build(args) => {
            require_safe_input(
                &joined_user_text(&args.query, [std::slice::from_ref(&args.purpose)]),
                config.region.as_deref(),
            )?;
            let profile_id = parse_id(&args.provider)?;
            let profile = vault
                .get_provider_profile(Actor::user(), &profile_id)?
                .ok_or(VaultError::NotFoundRecord)?;
            if !profile.enabled {
                return Err(CliError::Invalid("provider profile is disabled"));
            }
            let package = MethodPackage::load(&config.methods, &args.method)?;
            if !package.supports(&profile.capabilities) {
                return Err(CliError::Invalid(
                    "provider does not declare every capability required by this method",
                ));
            }
            let query_sensitivity: Sensitivity = args.query_sensitivity.into();
            if !profile.trusts(query_sensitivity) {
                return Err(CliError::Invalid(
                    "provider profile does not permit the request sensitivity",
                ));
            }
            let mut candidates = vault.search_memory(Actor::user(), &args.query, 100)?;
            for candidate in &mut candidates {
                candidate.policy_allowed = profile.trusts(candidate.sensitivity);
            }
            let packet = ContextBuilder::new(args.maximum_items)?.build(
                Actor::user(),
                args.purpose,
                args.method,
                profile.id,
                profile.model,
                args.query,
                query_sensitivity,
                candidates,
                Utc::now(),
            )?;
            vault.insert_context_packet(Actor::system(), &packet)?;
            print_value(config, &packet)
        }
        ContextCommand::Show(args) => {
            let packet = vault
                .get_context_packet(Actor::user(), &parse_id(&args.id)?)?
                .ok_or(VaultError::NotFoundRecord)?;
            print_value(config, &packet)
        }
        ContextCommand::Approve(args) => {
            let packet =
                vault.approve_context_packet(Actor::user(), &parse_id(&args.id)?, Utc::now())?;
            print_value(config, &packet)
        }
    }
}

async fn provider_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: ProviderCommand,
) -> Result<(), CliError> {
    match command {
        ProviderCommand::Add(args) => provider_add(config, vault, args),
        ProviderCommand::List => print_value(config, &vault.list_provider_profiles(Actor::user())?),
        ProviderCommand::Test(args) => {
            let profile = get_profile(vault, &args.id)?;
            let adapter = provider_adapter(config, profile.clone())?;
            let result = adapter.test_connection().await;
            vault.record_provider_test(Actor::user(), &profile.id, result.is_ok(), Utc::now())?;
            let result = result?;
            print_value(
                config,
                &json!({
                    "succeeded": result.succeeded,
                    "latency_ms": result.latency.as_millis()
                }),
            )
        }
        ProviderCommand::Enable(args) => set_provider_enabled(config, vault, &args.id, true),
        ProviderCommand::Disable(args) => set_provider_enabled(config, vault, &args.id, false),
        ProviderCommand::RevokeSecret(args) => revoke_provider_secret(config, vault, &args.id),
        ProviderCommand::Chat(args) => provider_chat(config, vault, args).await,
    }
}

fn set_provider_enabled(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    profile_id: &str,
    enabled: bool,
) -> Result<(), CliError> {
    let mut profile = get_profile(vault, profile_id)?;
    if enabled {
        if let Some(reference) = profile.credential_ref.as_deref() {
            if config.no_keyring {
                return Err(CliError::Invalid(
                    "provider credential access is disabled by --no-keyring",
                ));
            }
            let _credential_check = OsSecretStore.get_provider_secret(reference)?;
        }
    }
    if profile.enabled != enabled {
        profile.enabled = enabled;
        profile.updated_at = next_profile_update(profile.updated_at)?;
        vault.put_provider_profile(Actor::user(), &profile)?;
    }
    print_value(config, &profile)
}

fn revoke_provider_secret(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    profile_id: &str,
) -> Result<(), CliError> {
    if config.no_keyring {
        return Err(CliError::Invalid(
            "provider credential access is disabled by --no-keyring",
        ));
    }
    let mut profile = get_profile(vault, profile_id)?;
    let reference = profile
        .credential_ref
        .as_deref()
        .ok_or(CliError::Invalid(
            "provider profile has no credential reference",
        ))?
        .to_owned();
    if profile.enabled {
        profile.enabled = false;
        profile.updated_at = next_profile_update(profile.updated_at)?;
        vault.put_provider_profile(Actor::user(), &profile)?;
    }
    match OsSecretStore.delete_provider_secret(&reference) {
        Ok(()) | Err(VaultError::SecretNotFound) => {}
        Err(error) => return Err(error.into()),
    }
    print_value(config, &json!({"profile": profile, "secret_revoked": true}))
}

fn next_profile_update(previous: DateTime<Utc>) -> Result<DateTime<Utc>, CliError> {
    let now = Utc::now();
    if now > previous {
        Ok(now)
    } else {
        previous
            .checked_add_signed(chrono::Duration::milliseconds(1))
            .ok_or(CliError::Invalid("provider update timestamp overflow"))
    }
}

fn provider_add(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    args: ProviderAddArgs,
) -> Result<(), CliError> {
    let privacy: PrivacyKind = args.privacy.into();
    let needs_secret = privacy == PrivacyKind::Cloud || args.with_secret || args.secret_stdin;
    let now = Utc::now();
    let mut profile = ProviderProfile::new(
        args.name,
        args.kind.into(),
        args.base_url,
        args.model,
        privacy,
        ProviderCapabilities {
            streaming: args.streaming,
            tool_use: args.tool_use,
            json_schema: args.json_schema,
            vision: args.vision,
            context_limit: args.context_limit,
        },
        args.trusted_sensitivities
            .into_iter()
            .map(Sensitivity::from)
            .collect(),
        None,
        args.retention_note,
        now,
    );
    if needs_secret {
        profile.credential_ref = Some(format!("provider.{}", profile.id).to_ascii_lowercase());
    }
    profile.validate()?;

    if needs_secret && config.no_keyring {
        return Err(CliError::Invalid(
            "provider credentials require the OS credential store; remove --no-keyring or use an unauthenticated local endpoint",
        ));
    }

    let secret = needs_secret
        .then(|| read_secret(args.secret_stdin, "API credential: "))
        .transpose()?;
    if let (Some(reference), Some(secret)) = (&profile.credential_ref, &secret) {
        OsSecretStore.put_provider_secret(reference, secret.as_bytes())?;
    }
    if let Err(error) = vault.put_provider_profile(Actor::user(), &profile) {
        if let Some(reference) = &profile.credential_ref {
            let _cleanup = OsSecretStore.delete_provider_secret(reference);
        }
        return Err(error.into());
    }
    print_value(config, &profile)
}

fn method_command(config: &RuntimeConfig, command: MethodCommand) -> Result<(), CliError> {
    let selected = match &command {
        MethodCommand::List | MethodCommand::Validate(MethodValidateArgs { method: None }) => {
            OFFICIAL_METHOD_IDS
                .iter()
                .map(|method| (*method).to_owned())
                .collect::<Vec<_>>()
        }
        MethodCommand::Validate(MethodValidateArgs {
            method: Some(method),
        }) => vec![method.clone()],
    };
    let packages = selected
        .iter()
        .map(|method| MethodPackage::load(&config.methods, method))
        .collect::<Result<Vec<_>, _>>()?;
    let values = packages
        .iter()
        .map(|package| {
            json!({
                "id": package.identifier(),
                "title": package.manifest.title,
                "description": package.manifest.description,
                "review_status": package.manifest.review.status,
                "requires": package.manifest.requires,
                "examples": package.examples.len(),
                "evals": package.evals.len(),
                "valid": true
            })
        })
        .collect::<Vec<_>>();
    print_value(config, &values)
}

async fn provider_chat(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    args: ChatArgs,
) -> Result<(), CliError> {
    let context_id = parse_id(&args.context)?;
    let packet = vault
        .get_context_packet(Actor::user(), &context_id)?
        .ok_or(VaultError::NotFoundRecord)?;
    if packet.dispatched_at.is_some() {
        return Err(CliError::Invalid(
            "context packet was already dispatched; build a fresh packet for each provider call",
        ));
    }
    packet.authorize_egress()?;
    let profile = vault
        .get_provider_profile(Actor::user(), &packet.provider_profile_id)?
        .ok_or(VaultError::NotFoundRecord)?;
    let package = MethodPackage::load(&config.methods, &packet.method)?;
    if !package.supports(&profile.capabilities) {
        return Err(CliError::Invalid(
            "provider does not declare every capability required by this method",
        ));
    }
    let adapter = provider_adapter(config, profile.clone())?;
    let session = if let Some(identifier) = args.session.as_deref() {
        vault
            .get_session(Actor::user(), &parse_id(identifier)?)?
            .ok_or(VaultError::NotFoundRecord)?
    } else {
        require_safe_input(
            &joined_optional_user_text([args.title.as_deref()]),
            config.region.as_deref(),
        )?;
        let session = ReflectionSession::user_started(
            packet.method.clone(),
            args.title,
            packet.query_sensitivity,
            Utc::now(),
        );
        vault.create_session(Actor::user(), &session)?;
        session
    };
    packet.authorize_session_egress(&session, &profile)?;
    let user_sensitivity = session.sensitivity.max(packet.query_sensitivity);
    let assistant_sensitivity = packet
        .items
        .iter()
        .map(|item| item.sensitivity)
        .fold(user_sensitivity, Sensitivity::max);
    let user_message = SessionMessage::user(
        session.id.clone(),
        packet.query.clone(),
        user_sensitivity,
        Utc::now(),
    );
    vault.append_session_message(Actor::user(), &user_message)?;

    let provenance = Source::model(
        packet.method.clone(),
        profile.name.clone(),
        profile.model.clone(),
        packet.id.clone(),
    );
    let dispatched =
        vault.mark_context_packet_dispatched(Actor::user(), &context_id, Utc::now())?;
    let request = GenerationRequest {
        context: dispatched,
        instructions: package.provider_instructions(),
        output: OutputContract::Text,
        max_output_tokens: args.max_output_tokens,
    };
    if args.stream {
        let mut stream = adapter.stream(request).await?;
        let mut stdout = io::stdout().lock();
        if !config.json {
            eprintln!("Quisum session: {}", session.id);
        }
        while let Some(event) = stream.next().await {
            match event? {
                ProviderEvent::TextDelta(delta) if config.json => {
                    serde_json::to_writer(
                        &mut stdout,
                        &json!({
                            "event": "text_delta",
                            "session_id": session.id,
                            "text": delta
                        }),
                    )?;
                    stdout.write_all(b"\n")?;
                }
                ProviderEvent::TextDelta(delta) => {
                    stdout.write_all(delta.as_bytes())?;
                    stdout.flush()?;
                }
                ProviderEvent::Completed(response) if config.json => {
                    let assistant = SessionMessage::assistant(
                        session.id.clone(),
                        response.text.clone(),
                        assistant_sensitivity,
                        provenance.clone(),
                        Utc::now(),
                    );
                    vault.append_session_message(Actor::user(), &assistant)?;
                    serde_json::to_writer(
                        &mut stdout,
                        &json!({
                            "event": "completed",
                            "session_id": session.id,
                            "message_id": assistant.id,
                            "response": response
                        }),
                    )?;
                    stdout.write_all(b"\n")?;
                }
                ProviderEvent::Completed(response) => {
                    let assistant = SessionMessage::assistant(
                        session.id.clone(),
                        response.text,
                        assistant_sensitivity,
                        provenance.clone(),
                        Utc::now(),
                    );
                    vault.append_session_message(Actor::user(), &assistant)?;
                    stdout.write_all(b"\n")?;
                }
            }
        }
        Ok(())
    } else {
        let response = adapter.generate(request).await?;
        let assistant = SessionMessage::assistant(
            session.id.clone(),
            response.text.clone(),
            assistant_sensitivity,
            provenance,
            Utc::now(),
        );
        vault.append_session_message(Actor::user(), &assistant)?;
        print_value(
            config,
            &json!({
                "session_id": session.id,
                "message_id": assistant.id,
                "response": response
            }),
        )
    }
}

fn experiment_command(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    command: ExperimentCommand,
) -> Result<(), CliError> {
    match command {
        ExperimentCommand::Propose(args) => {
            let mut safety_text = joined_user_text(
                &args.title,
                [
                    std::slice::from_ref(&args.hypothesis),
                    std::slice::from_ref(&args.prediction),
                    std::slice::from_ref(&args.action),
                    std::slice::from_ref(&args.stop_condition),
                ],
            );
            append_user_text(&mut safety_text, args.expected_result.as_deref());
            require_safe_input(&safety_text, config.region.as_deref())?;
            let mut experiment = BehavioralExperiment::proposal(
                args.title,
                args.hypothesis,
                args.prediction,
                args.action,
                args.stop_condition,
                args.risk.into(),
                Utc::now(),
                args.sensitivity.into(),
                Source::user(),
            );
            experiment.related_claims = parse_id_set(args.related_claims)?;
            experiment.expected_result = args.expected_result;
            experiment.validate()?;
            vault.insert_experiment(Actor::user(), &experiment)?;
            print_value(config, &experiment)
        }
        ExperimentCommand::List(args) => {
            print_value(config, &vault.list_experiments(Actor::user(), args.limit)?)
        }
        ExperimentCommand::Show(args) => {
            let experiment = vault
                .get_experiment(Actor::user(), &parse_id(&args.id)?)?
                .ok_or(VaultError::NotFoundRecord)?;
            print_value(config, &experiment)
        }
        ExperimentCommand::Activate(args) => {
            let experiment =
                vault.activate_experiment(Actor::user(), &parse_id(&args.id)?, Utc::now())?;
            print_value(config, &experiment)
        }
        ExperimentCommand::Complete(args) => {
            require_safe_input(
                &joined_optional_user_text([
                    Some(args.actual_result.as_str()),
                    Some(args.learning.as_str()),
                ]),
                config.region.as_deref(),
            )?;
            let experiment = vault.complete_experiment(
                Actor::user(),
                &parse_id(&args.id)?,
                args.actual_result,
                args.learning,
                Utc::now(),
            )?;
            print_value(config, &experiment)
        }
        ExperimentCommand::Cancel(args) => {
            let experiment =
                vault.cancel_experiment(Actor::user(), &parse_id(&args.id)?, Utc::now())?;
            print_value(config, &experiment)
        }
    }
}

fn reflog(config: &RuntimeConfig, vault: &SqlCipherVault, args: LimitArgs) -> Result<(), CliError> {
    let entries = vault
        .list_reflog(Actor::user(), args.limit)?
        .into_iter()
        .map(|entry| json!({"sequence": entry.sequence, "event": entry.event}))
        .collect::<Vec<_>>();
    print_value(config, &entries)
}

fn export(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    args: ExportArgs,
) -> Result<(), CliError> {
    if !args.acknowledge_plaintext {
        return Err(CliError::Invalid(
            "portable exports are plaintext; repeat with --acknowledge-plaintext or use backup for encrypted recovery",
        ));
    }
    match args.format {
        PortableFormatArg::Json => print_value(
            config,
            &vault.export_portable(
                Actor::user(),
                args.destination,
                ExportFormat::Json,
                Utc::now(),
            )?,
        ),
        PortableFormatArg::Jsonl => print_value(
            config,
            &vault.export_portable(
                Actor::user(),
                args.destination,
                ExportFormat::Jsonl,
                Utc::now(),
            )?,
        ),
        PortableFormatArg::Markdown => print_value(
            config,
            &vault.export_markdown(Actor::user(), args.destination, Utc::now())?,
        ),
    }
}

fn import(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    args: ImportArgs,
) -> Result<(), CliError> {
    let receipt = vault.import_portable(Actor::user(), args.source, Utc::now())?;
    print_value(config, &receipt)
}

fn backup(
    config: &RuntimeConfig,
    vault: &SqlCipherVault,
    args: BackupArgs,
) -> Result<(), CliError> {
    let passphrase = read_new_passphrase(
        args.recovery_passphrase_stdin,
        "New backup recovery passphrase: ",
        "Repeat backup recovery passphrase: ",
    )?;
    let receipt = vault.create_backup(Actor::user(), args.destination, &passphrase, Utc::now())?;
    print_value(config, &receipt)
}

fn restore(config: &RuntimeConfig, args: RestoreArgs) -> Result<(), CliError> {
    require_safe_vault_location(&args.destination, args.allow_git_directory)?;
    let recovery_path = if config.recovery == default_recovery_path(&config.vault) {
        default_recovery_path(&args.destination)
    } else {
        config.recovery.clone()
    };
    if recovery_path.exists() {
        return Err(CliError::Invalid(
            "recovery path already exists; Quisum never overwrites recovery material",
        ));
    }
    let passphrase = read_secret(
        args.recovery_passphrase_stdin,
        "Backup recovery passphrase: ",
    )?;
    let restored = SqlCipherVault::restore_backup(
        Actor::user(),
        args.source,
        &args.destination,
        &passphrase,
        Utc::now(),
    )?;
    let restored = RestoredSetupGuard::new(restored, recovery_path.clone());
    let envelope = RecoveryKeyEnvelope::seal(&restored.get()?.key, &passphrase)?;
    write_private_json_new(&recovery_path, &envelope)?;
    let vault_id = restored.get()?.vault.vault_id()?;
    let restored = restored.commit()?;
    let keyring_saved = if config.no_keyring {
        false
    } else {
        OsSecretStore
            .put_vault_key(&args.destination, &restored.key)
            .is_ok()
    };
    print_value(
        config,
        &json!({
            "vault_id": vault_id,
            "vault_path": args.destination,
            "recovery_path": recovery_path,
            "keyring_saved": keyring_saved
        }),
    )
}

async fn serve_mcp(config: &RuntimeConfig) -> Result<(), CliError> {
    if config.no_keyring || config.unlock_passphrase_stdin {
        return Err(CliError::Invalid(
            "MCP stdio requires OS-keyring unlock; stdin cannot carry a recovery passphrase and MCP frames simultaneously",
        ));
    }
    let key = OsSecretStore.get_vault_key(&config.vault)?;
    let vault = SqlCipherVault::open(&config.vault, &key)?;
    quisum_mcp::serve_stdio(Arc::new(vault))
        .await
        .map_err(|_| CliError::Invalid("MCP transport failed"))
}

fn unlock_vault(config: &RuntimeConfig) -> Result<SqlCipherVault, CliError> {
    if !config.no_keyring {
        if let Ok(key) = OsSecretStore.get_vault_key(&config.vault) {
            if let Ok(vault) = SqlCipherVault::open(&config.vault, &key) {
                return Ok(vault);
            }
        }
    }
    let envelope = read_recovery_envelope(&config.recovery)?;
    let passphrase = read_secret(
        config.unlock_passphrase_stdin,
        "Vault recovery passphrase: ",
    )?;
    let key = envelope.open(&passphrase)?;
    SqlCipherVault::open(&config.vault, &key).map_err(Into::into)
}

fn get_profile(vault: &SqlCipherVault, identifier: &str) -> Result<ProviderProfile, CliError> {
    vault
        .get_provider_profile(Actor::user(), &parse_id(identifier)?)?
        .ok_or_else(|| VaultError::NotFoundRecord.into())
}

fn provider_adapter(
    config: &RuntimeConfig,
    profile: ProviderProfile,
) -> Result<HttpProvider, CliError> {
    if config.no_keyring && profile.credential_ref.is_some() {
        return Err(CliError::Invalid(
            "provider credential access is disabled by --no-keyring",
        ));
    }
    let credential = profile
        .credential_ref
        .as_deref()
        .map(|reference| {
            let bytes = OsSecretStore.get_provider_secret(reference)?;
            ProviderCredential::new(bytes.to_vec()).map_err(CliError::from)
        })
        .transpose()?;
    HttpProvider::new(profile, credential).map_err(Into::into)
}

fn require_safe_input(input: &str, region: Option<&str>) -> Result<(), CliError> {
    let decision = SafetyEngine.screen_user_input(input);
    match decision.route {
        SafetyRoute::Continue if decision.allow_persistence => Ok(()),
        SafetyRoute::Crisis => {
            let message = match CrisisResourcePack::embedded() {
                Ok(pack) => {
                    let guidance = pack.guidance(region);
                    if guidance.source_urls.is_empty() {
                        guidance.assistant_response
                    } else {
                        format!(
                            "{} Sources: {}",
                            guidance.assistant_response,
                            guidance.source_urls.join(", ")
                        )
                    }
                }
                Err(_) => "Quisum is not an emergency service. Stop identity analysis. If anyone may be in immediate danger, contact local emergency services now and contact a trusted person who can help without putting themselves in danger.".to_owned(),
            };
            Err(CliError::Safety(message))
        }
        SafetyRoute::RefuseThirdPartyProfiling => Err(CliError::Safety(
            "Quisum refuses covert profiling of another person; reflect on your own observations, needs, boundaries, and choices instead.".to_owned(),
        )),
        SafetyRoute::Pause | SafetyRoute::Continue => Err(CliError::Safety(
            "Safety routing paused identity analysis and durable persistence.".to_owned(),
        )),
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

fn parse_id(value: &str) -> Result<EntityId, CliError> {
    value.parse().map_err(Into::into)
}

fn parse_id_set(values: Vec<String>) -> Result<BTreeSet<EntityId>, CliError> {
    values.into_iter().map(|value| parse_id(&value)).collect()
}

fn parse_time(value: &str) -> Result<DateTime<Utc>, CliError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| CliError::Invalid("timestamp must be RFC 3339"))
}

fn default_vault_path() -> Result<PathBuf, CliError> {
    let directories = ProjectDirs::from("org", "Quisum", "Quisum").ok_or(CliError::Invalid(
        "could not resolve a platform data directory",
    ))?;
    Ok(directories.data_local_dir().join("vault.db"))
}

fn default_methods_path() -> PathBuf {
    if let Ok(current) = std::env::current_dir() {
        let candidate = current.join("methods");
        if candidate.is_dir() {
            return candidate;
        }
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let candidate = parent.join("../share/quisum/methods");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../methods")
}

fn default_recovery_path(vault: &Path) -> PathBuf {
    let mut name = vault
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "vault.db".into());
    name.push(".recovery.json");
    vault.with_file_name(name)
}

fn require_safe_vault_location(path: &Path, allow_git: bool) -> Result<(), CliError> {
    if allow_git {
        return Ok(());
    }
    let mut cursor = path.parent();
    while let Some(directory) = cursor {
        if directory.join(".git").exists() {
            return Err(CliError::Invalid(
                "refusing to place vault artifacts in a Git work tree without --allow-git-directory",
            ));
        }
        cursor = directory.parent();
    }
    Ok(())
}

fn read_new_passphrase(
    from_stdin: bool,
    first_prompt: &str,
    second_prompt: &str,
) -> Result<Zeroizing<String>, CliError> {
    let first = read_secret(from_stdin, first_prompt)?;
    let second = read_secret(from_stdin, second_prompt)?;
    if first.as_str() != second.as_str() {
        return Err(CliError::Invalid("recovery passphrases did not match"));
    }
    Ok(first)
}

fn read_secret(from_stdin: bool, prompt: &str) -> Result<Zeroizing<String>, CliError> {
    let value = if from_stdin {
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        line
    } else {
        if !io::stdin().is_terminal() {
            return Err(CliError::Invalid(
                "stdin is not a terminal; use the explicit stdin secret flag",
            ));
        }
        rpassword::prompt_password(prompt)?
    };
    if value.is_empty() || value.len() > MAX_SECRET_BYTES || value.as_bytes().contains(&0) {
        return Err(CliError::Invalid(
            "secret input is empty or exceeds the safe size limit",
        ));
    }
    Ok(Zeroizing::new(value))
}

fn read_recovery_envelope(path: &Path) -> Result<RecoveryKeyEnvelope, CliError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_RECOVERY_FILE_BYTES {
        return Err(CliError::Invalid(
            "recovery envelope is not a bounded regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CliError::Invalid(
                "recovery envelope permissions must not allow group or other access",
            ));
        }
    }
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_RECOVERY_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_err(|_| CliError::Invalid("recovery file is too large"))?
        > MAX_RECOVERY_FILE_BYTES
    {
        return Err(CliError::Invalid("recovery file is too large"));
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn write_private_json_new<T: Serialize>(path: &Path, value: &T) -> Result<(), CliError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let options = &mut OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let mut guard = NewPrivateFileGuard::new(path.to_path_buf());
    let mut writer = io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    guard.commit();
    Ok(())
}

struct NewPrivateFileGuard {
    path: PathBuf,
    committed: bool,
}

impl NewPrivateFileGuard {
    const fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    const fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for NewPrivateFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct RestoredSetupGuard {
    restored: Option<RestoredVault>,
    destination: PathBuf,
    recovery_path: PathBuf,
}

impl RestoredSetupGuard {
    fn new(restored: RestoredVault, recovery_path: PathBuf) -> Self {
        let destination = restored.vault.path().to_path_buf();
        Self {
            restored: Some(restored),
            destination,
            recovery_path,
        }
    }

    fn get(&self) -> Result<&RestoredVault, CliError> {
        self.restored
            .as_ref()
            .ok_or(CliError::Invalid("restored vault setup guard is empty"))
    }

    fn commit(mut self) -> Result<RestoredVault, CliError> {
        self.restored
            .take()
            .ok_or(CliError::Invalid("restored vault setup guard is empty"))
    }
}

impl Drop for RestoredSetupGuard {
    fn drop(&mut self) {
        let restored = self.restored.take();
        if restored.is_none() {
            return;
        }
        drop(restored);
        let _ = fs::remove_file(&self.recovery_path);
        let _ = fs::remove_file(&self.destination);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = OsString::from(self.destination.as_os_str());
            sidecar.push(suffix);
            let _ = fs::remove_file(PathBuf::from(sidecar));
        }
    }
}

fn print_value<T: Serialize>(config: &RuntimeConfig, value: &T) -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    if config.json {
        serde_json::to_writer(&mut stdout, value)?;
    } else {
        serde_json::to_writer_pretty(&mut stdout, value)?;
    }
    stdout.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    struct FailingJson;

    impl Serialize for FailingJson {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let mut sequence = serializer.serialize_seq(None)?;
            serde::ser::SerializeSeq::serialize_element(&mut sequence, "partial")?;
            Err(<S::Error as serde::ser::Error>::custom(
                "synthetic serialization failure",
            ))
        }
    }

    #[test]
    fn secret_values_are_not_accepted_as_command_line_arguments() {
        let help = Cli::command().render_long_help().to_string();
        assert!(!help.contains("--api-key"));
        assert!(!help.contains("--passphrase "));
        assert!(help.contains("--unlock-passphrase-stdin"));
    }

    #[test]
    fn git_work_tree_storage_requires_an_explicit_override()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::create_dir(directory.path().join(".git"))?;
        let vault = directory.path().join("private/vault.db");
        assert!(require_safe_vault_location(&vault, false).is_err());
        assert!(require_safe_vault_location(&vault, true).is_ok());
        Ok(())
    }

    #[test]
    fn failed_private_json_write_removes_partial_artifact() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("recovery.json");
        assert!(write_private_json_new(&path, &FailingJson).is_err());
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn failed_recovery_setup_removes_the_new_restored_vault()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let destination = directory.path().join("restored.db");
        let recovery_path = directory.path().join("restored.recovery.json");
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(&destination, &key)?;

        let result: Result<(), CliError> = (|| {
            let _restored =
                RestoredSetupGuard::new(RestoredVault { vault, key }, recovery_path.clone());
            write_private_json_new(&recovery_path, &FailingJson)?;
            Ok(())
        })();

        assert!(result.is_err());
        assert!(!recovery_path.exists());
        assert!(!destination.exists());
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = OsString::from(destination.as_os_str());
            sidecar.push(suffix);
            assert!(!PathBuf::from(sidecar).exists());
        }
        Ok(())
    }

    #[test]
    fn provider_update_timestamp_always_advances() -> Result<(), Box<dyn std::error::Error>> {
        let previous = Utc::now() + chrono::Duration::hours(1);
        assert_eq!(
            next_profile_update(previous)?,
            previous + chrono::Duration::milliseconds(1)
        );
        Ok(())
    }
}
