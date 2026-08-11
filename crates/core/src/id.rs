use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use ulid::Ulid;

use crate::DomainError;

/// Kinds used as stable prefixes in public entity identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityKind {
    /// A vault.
    Vault,
    /// A reflection session.
    Session,
    /// A message.
    Message,
    /// A session summary.
    SessionSummary,
    /// A concrete event.
    Event,
    /// A claim.
    Claim,
    /// A claim revision.
    Revision,
    /// A human review.
    Review,
    /// A user correction.
    Correction,
    /// An open question.
    OpenQuestion,
    /// A behavioral experiment.
    Experiment,
    /// A context packet.
    Context,
    /// A provider profile.
    Provider,
    /// A consent record.
    Consent,
    /// An audit record.
    Audit,
    /// A formal assessment.
    Assessment,
    /// A portable export.
    Export,
    /// An encrypted backup.
    Backup,
    /// An import operation.
    Import,
}

impl EntityKind {
    /// Return the canonical string prefix.
    #[must_use]
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Vault => "vault",
            Self::Session => "session",
            Self::Message => "message",
            Self::SessionSummary => "session_summary",
            Self::Event => "event",
            Self::Claim => "claim",
            Self::Revision => "revision",
            Self::Review => "review",
            Self::Correction => "correction",
            Self::OpenQuestion => "open_question",
            Self::Experiment => "experiment",
            Self::Context => "context",
            Self::Provider => "provider",
            Self::Consent => "consent",
            Self::Audit => "audit",
            Self::Assessment => "assessment",
            Self::Export => "export",
            Self::Backup => "backup",
            Self::Import => "import",
        }
    }

    fn from_prefix(prefix: &str) -> Result<Self, DomainError> {
        match prefix {
            "vault" => Ok(Self::Vault),
            "session" => Ok(Self::Session),
            "message" => Ok(Self::Message),
            "session_summary" => Ok(Self::SessionSummary),
            "event" => Ok(Self::Event),
            "claim" => Ok(Self::Claim),
            "revision" => Ok(Self::Revision),
            "review" => Ok(Self::Review),
            "correction" => Ok(Self::Correction),
            "open_question" => Ok(Self::OpenQuestion),
            "experiment" => Ok(Self::Experiment),
            "context" => Ok(Self::Context),
            "provider" => Ok(Self::Provider),
            "consent" => Ok(Self::Consent),
            "audit" => Ok(Self::Audit),
            "assessment" => Ok(Self::Assessment),
            "export" => Ok(Self::Export),
            "backup" => Ok(Self::Backup),
            "import" => Ok(Self::Import),
            _ => Err(DomainError::InvalidEntityId),
        }
    }
}

/// A sortable public identifier with an explicit entity-kind prefix.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId {
    kind: EntityKind,
    value: Ulid,
}

impl EntityId {
    /// Generate a new identifier for an entity kind.
    #[must_use]
    pub fn new(kind: EntityKind) -> Self {
        Self {
            kind,
            value: Ulid::generate(),
        }
    }

    /// Build an identifier from a known ULID.
    #[must_use]
    pub const fn from_ulid(kind: EntityKind, value: Ulid) -> Self {
        Self { kind, value }
    }

    /// Return the identifier's kind.
    #[must_use]
    pub const fn kind(&self) -> EntityKind {
        self.kind
    }

    /// Require a particular identifier kind.
    pub fn require_kind(&self, expected: EntityKind) -> Result<(), DomainError> {
        if self.kind == expected {
            Ok(())
        } else {
            Err(DomainError::WrongEntityKind {
                expected,
                actual: self.kind,
            })
        }
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}_{}", self.kind.prefix(), self.value)
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl FromStr for EntityId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !value.is_ascii() || value.len() < 28 {
            return Err(DomainError::InvalidEntityId);
        }
        let separator = value.len() - 27;
        if value.as_bytes().get(separator) != Some(&b'_') {
            return Err(DomainError::InvalidEntityId);
        }
        let kind = EntityKind::from_prefix(&value[..separator])?;
        let ulid =
            Ulid::from_string(&value[separator + 1..]).map_err(|_| DomainError::InvalidEntityId)?;
        Ok(Self::from_ulid(kind, ulid))
    }
}

impl Serialize for EntityId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EntityId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn round_trips_all_prefix_shapes() -> Result<(), Box<dyn Error>> {
        for kind in [
            EntityKind::Event,
            EntityKind::Claim,
            EntityKind::OpenQuestion,
            EntityKind::SessionSummary,
        ] {
            let id = EntityId::new(kind);
            let parsed: EntityId = id.to_string().parse()?;
            assert_eq!(parsed, id);
        }
        Ok(())
    }

    #[test]
    fn rejects_unknown_and_noncanonical_ids() {
        assert!(
            "unknown_01J50000000000000000000001"
                .parse::<EntityId>()
                .is_err()
        );
        assert!("event_not-a-ulid".parse::<EntityId>().is_err());
        assert!(
            "event01J50000000000000000000001"
                .parse::<EntityId>()
                .is_err()
        );
    }
}
