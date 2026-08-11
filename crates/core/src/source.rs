use serde::{Deserialize, Serialize};

use crate::{
    DomainError, EntityId, EntityKind, Validate,
    validation::{validate_method_id, validate_text},
};

/// Provenance actor for a durable domain item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceActor {
    /// Directly authored or explicitly captured by the user.
    User,
    /// Proposed by a model.
    Model,
    /// Read from a portable import.
    Import,
    /// Created by deterministic system logic.
    System,
}

/// Provider-neutral provenance kept with generated and imported data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Source actor.
    pub actor: SourceActor,
    /// Method identifier and semantic version for model output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Provider profile/name recorded at generation time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Provider model identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Immutable context packet sent for generation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_packet_id: Option<EntityId>,
    /// Import operation that introduced the item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_id: Option<EntityId>,
}

impl Source {
    /// Direct user provenance.
    #[must_use]
    pub const fn user() -> Self {
        Self {
            actor: SourceActor::User,
            method: None,
            provider: None,
            model: None,
            context_packet_id: None,
            import_id: None,
        }
    }

    /// Deterministic system provenance.
    #[must_use]
    pub const fn system() -> Self {
        Self {
            actor: SourceActor::System,
            method: None,
            provider: None,
            model: None,
            context_packet_id: None,
            import_id: None,
        }
    }

    /// Model-generated provenance.
    #[must_use]
    pub fn model(
        method: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        context_packet_id: EntityId,
    ) -> Self {
        Self {
            actor: SourceActor::Model,
            method: Some(method.into()),
            provider: Some(provider.into()),
            model: Some(model.into()),
            context_packet_id: Some(context_packet_id),
            import_id: None,
        }
    }

    /// Validated import provenance.
    #[must_use]
    pub fn import(import_id: EntityId) -> Self {
        Self {
            actor: SourceActor::Import,
            method: None,
            provider: None,
            model: None,
            context_packet_id: None,
            import_id: Some(import_id),
        }
    }
}

impl Validate for Source {
    fn validate(&self) -> Result<(), DomainError> {
        match self.actor {
            SourceActor::Model => {
                let method = self
                    .method
                    .as_deref()
                    .ok_or(DomainError::InvalidValue("source.method"))?;
                let provider = self
                    .provider
                    .as_deref()
                    .ok_or(DomainError::InvalidValue("source.provider"))?;
                let model = self
                    .model
                    .as_deref()
                    .ok_or(DomainError::InvalidValue("source.model"))?;
                validate_method_id(method, "source.method")?;
                validate_text(provider, 120, "source.provider")?;
                validate_text(model, 160, "source.model")?;
                self.context_packet_id
                    .as_ref()
                    .ok_or(DomainError::InvalidValue("source.context_packet_id"))?
                    .require_kind(EntityKind::Context)?;
                if self.import_id.is_some() {
                    return Err(DomainError::InvalidValue("source.import_id"));
                }
            }
            SourceActor::Import => {
                self.import_id
                    .as_ref()
                    .ok_or(DomainError::InvalidValue("source.import_id"))?
                    .require_kind(EntityKind::Import)?;
                if self.method.is_some()
                    || self.provider.is_some()
                    || self.model.is_some()
                    || self.context_packet_id.is_some()
                {
                    return Err(DomainError::InvalidValue("source.model_fields"));
                }
            }
            SourceActor::User | SourceActor::System => {
                if self.method.is_some()
                    || self.provider.is_some()
                    || self.model.is_some()
                    || self.context_packet_id.is_some()
                    || self.import_id.is_some()
                {
                    return Err(DomainError::InvalidValue("source.extra_fields"));
                }
            }
        }
        Ok(())
    }
}
