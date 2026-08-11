#![doc = "Domain policy, authorization, lifecycle, context, and safety for Quisum."]
#![forbid(unsafe_code)]

mod actor;
mod audit;
mod claim;
mod context;
mod correction;
mod error;
mod event;
mod experiment;
mod id;
mod method;
mod open_question;
mod provider;
mod safety;
mod sensitivity;
mod session;
mod source;
mod validation;

pub use actor::{Actor, ActorKind, Capability};
pub use audit::{AuditAction, AuditActor, AuditDetails, AuditEntityType, AuditEvent};
pub use claim::{
    Claim, ClaimKind, ClaimRevision, ClaimRevisionKind, ClaimSnapshot, ClaimStatus, EvidenceGrade,
    ReviewDecision, ReviewOutcome, ReviewRequest, UserEndorsement, UserReview,
};
pub use context::{
    ContextBuilder, ContextExclusion, ContextItem, ContextItemType, ContextPacket, ExclusionReason,
    MemoryCandidate, MemoryState,
};
pub use correction::{CorrectionScope, CorrectionScopeKind, UserCorrection};
pub use error::DomainError;
pub use event::{Event, EventStatus};
pub use experiment::{BehavioralExperiment, ExperimentRisk, ExperimentStatus};
pub use id::{EntityId, EntityKind};
pub use method::{
    MethodError, MethodFiles, MethodManifest, MethodMemoryPolicy, MethodPackage,
    MethodRequirements, MethodRetrievalPolicy, MethodReview, MethodReviewStatus,
    OFFICIAL_METHOD_IDS,
};
pub use open_question::{OpenQuestion, OpenQuestionStatus};
pub use provider::{PrivacyKind, ProviderCapabilities, ProviderKind, ProviderProfile};
pub use safety::{SafetyDecision, SafetyEngine, SafetyReason, SafetyRoute};
pub use sensitivity::Sensitivity;
pub use session::{MessageRole, ReflectionSession, SessionMessage, SessionStatus};
pub use source::{Source, SourceActor};
pub use validation::Validate;

/// Stable product name.
pub const PRODUCT_NAME: &str = "Quisum";
/// Stable public schema version implemented by this crate.
pub const SCHEMA_VERSION: &str = "1.0.0";
