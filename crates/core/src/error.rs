use crate::{Capability, ClaimStatus, EntityKind};

/// Errors produced by domain validation and authorization.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    /// An entity identifier is malformed or has an unknown prefix.
    #[error("invalid entity identifier")]
    InvalidEntityId,
    /// A required field or invariant is invalid.
    #[error("invalid domain value: {0}")]
    InvalidValue(&'static str),
    /// An actor attempted an operation outside its capability set.
    #[error("actor is not allowed to use capability {0:?}")]
    Forbidden(Capability),
    /// A claim cannot move between the supplied lifecycle states.
    #[error("invalid claim transition from {from:?} to {to:?}")]
    InvalidClaimTransition {
        /// Current state.
        from: ClaimStatus,
        /// Requested state.
        to: ClaimStatus,
    },
    /// An identifier had the wrong entity kind for an operation.
    #[error("expected entity kind {expected:?}, found {actual:?}")]
    WrongEntityKind {
        /// Required kind.
        expected: EntityKind,
        /// Actual kind.
        actual: EntityKind,
    },
    /// A safety gate prohibited persistence or continued analysis.
    #[error("operation blocked by safety policy")]
    SafetyBlocked,
}
