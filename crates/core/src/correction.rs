use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    DomainError, EntityId, EntityKind, Sensitivity, Source, SourceActor, Validate,
    validation::{validate_text, validate_topic},
};

/// Machine-readable correction scope kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionScopeKind {
    /// Suppress or replace an interpretation.
    Interpretation,
    /// Suppress a phrase or label.
    Phrase,
    /// Apply within a topic.
    Topic,
    /// Apply to a specific claim.
    Claim,
    /// Apply globally.
    Global,
}

/// Scope of a user correction that must outrank model inference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrectionScope {
    /// Scope kind.
    pub kind: CorrectionScopeKind,
    /// Required bounded value except for global scope.
    pub value: Option<String>,
}

impl CorrectionScope {
    /// Scope a correction to an interpretation.
    #[must_use]
    pub fn interpretation(value: impl Into<String>) -> Self {
        Self {
            kind: CorrectionScopeKind::Interpretation,
            value: Some(value.into()),
        }
    }

    /// Scope a correction to a phrase or label.
    #[must_use]
    pub fn phrase(value: impl Into<String>) -> Self {
        Self {
            kind: CorrectionScopeKind::Phrase,
            value: Some(value.into()),
        }
    }

    /// Scope a correction to a topic.
    #[must_use]
    pub fn topic(value: impl Into<String>) -> Self {
        Self {
            kind: CorrectionScopeKind::Topic,
            value: Some(value.into()),
        }
    }

    /// Scope a correction to one claim.
    #[must_use]
    pub fn claim(claim_id: &EntityId) -> Self {
        Self {
            kind: CorrectionScopeKind::Claim,
            value: Some(claim_id.to_string()),
        }
    }

    /// Apply a correction globally.
    #[must_use]
    pub const fn global() -> Self {
        Self {
            kind: CorrectionScopeKind::Global,
            value: None,
        }
    }
}

/// Direct user correction with retrieval priority over model-derived claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCorrection {
    /// Public schema version.
    pub schema_version: String,
    /// Correction identifier.
    pub id: EntityId,
    /// User-authored correction statement.
    pub statement: String,
    /// Scope.
    pub scope: CorrectionScope,
    /// Related claims.
    pub related_claims: BTreeSet<EntityId>,
    /// Related topics.
    pub topics: BTreeSet<String>,
    /// Whether the correction remains active.
    pub active: bool,
    /// Sensitivity policy.
    pub sensitivity: Sensitivity,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last update time.
    pub updated_at: DateTime<Utc>,
    /// User provenance.
    pub source: Source,
}

impl UserCorrection {
    /// Create an active correction directly authored by the user.
    #[must_use]
    pub fn new(
        statement: impl Into<String>,
        scope: CorrectionScope,
        sensitivity: Sensitivity,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Correction),
            statement: statement.into(),
            scope,
            related_claims: BTreeSet::new(),
            topics: BTreeSet::new(),
            active: true,
            sensitivity,
            created_at: now,
            updated_at: now,
            source: Source::user(),
        }
    }
}

impl Validate for UserCorrection {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("correction.schema_version"));
        }
        self.id.require_kind(EntityKind::Correction)?;
        validate_text(&self.statement, 4_000, "correction.statement")?;
        match self.scope.kind {
            CorrectionScopeKind::Interpretation
            | CorrectionScopeKind::Phrase
            | CorrectionScopeKind::Topic => {
                validate_text(
                    self.scope
                        .value
                        .as_deref()
                        .ok_or(DomainError::InvalidValue("correction.scope"))?,
                    1_000,
                    "correction.scope",
                )?;
            }
            CorrectionScopeKind::Claim => {
                let claim_id = self
                    .scope
                    .value
                    .as_deref()
                    .ok_or(DomainError::InvalidValue("correction.scope"))?
                    .parse::<EntityId>()?;
                claim_id.require_kind(EntityKind::Claim)?;
            }
            CorrectionScopeKind::Global => {
                if self.scope.value.is_some() {
                    return Err(DomainError::InvalidValue("correction.scope"));
                }
            }
        }
        if self.related_claims.len() > 64 || self.topics.len() > 32 {
            return Err(DomainError::InvalidValue("correction.links"));
        }
        for claim in &self.related_claims {
            claim.require_kind(EntityKind::Claim)?;
        }
        for topic in &self.topics {
            validate_topic(topic, "correction.topics")?;
        }
        if self.updated_at < self.created_at {
            return Err(DomainError::InvalidValue("correction.updated_at"));
        }
        self.source.validate()?;
        if self.source.actor != SourceActor::User {
            return Err(DomainError::InvalidValue("correction.source"));
        }
        Ok(())
    }
}
