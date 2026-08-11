use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Actor, ActorKind, Capability, DomainError, EntityId, EntityKind, Sensitivity, Source,
    SourceActor, Validate,
    validation::{validate_method_id, validate_text},
};

/// Lifecycle of one bounded reflection session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Accepting new messages.
    Active,
    /// Explicitly completed by the user.
    Completed,
    /// Closed without requiring a conclusion.
    Abandoned,
    /// Content deleted by an authorized local surface.
    Deleted,
}

/// A user-owned reflection session tied to one versioned method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionSession {
    /// Public schema version.
    pub schema_version: String,
    /// Session identifier.
    pub id: EntityId,
    /// Versioned method identifier.
    pub method: String,
    /// Optional user-facing title.
    pub title: Option<String>,
    /// Lifecycle state.
    pub status: SessionStatus,
    /// Creation time.
    pub started_at: DateTime<Utc>,
    /// Terminal transition time.
    pub completed_at: Option<DateTime<Utc>>,
    /// Default transcript sensitivity.
    pub sensitivity: Sensitivity,
    /// Acute-crisis content is not ordinary identity memory.
    pub crisis_related: bool,
    /// Session provenance.
    pub source: Source,
}

impl ReflectionSession {
    /// Start a direct user session.
    #[must_use]
    pub fn user_started(
        method: impl Into<String>,
        title: Option<String>,
        sensitivity: Sensitivity,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Session),
            method: method.into(),
            title,
            status: SessionStatus::Active,
            started_at,
            completed_at: None,
            sensitivity,
            crisis_related: false,
            source: Source::user(),
        }
    }

    /// Close an active session, either completed or abandoned.
    pub fn close(
        &mut self,
        actor: Actor,
        completed: bool,
        closed_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        actor.require(Capability::ManageSession)?;
        if actor.kind() != ActorKind::User || self.status != SessionStatus::Active {
            return Err(DomainError::Forbidden(Capability::ManageSession));
        }
        self.validate()?;
        let mut updated = self.clone();
        updated.status = if completed {
            SessionStatus::Completed
        } else {
            SessionStatus::Abandoned
        };
        updated.completed_at = Some(closed_at);
        updated.validate()?;
        *self = updated;
        Ok(())
    }
}

impl Validate for ReflectionSession {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("session.schema_version"));
        }
        self.id.require_kind(EntityKind::Session)?;
        validate_method_id(&self.method, "session.method")?;
        if let Some(title) = &self.title {
            validate_text(title, 240, "session.title")?;
        }
        match self.status {
            SessionStatus::Active => {
                if self.completed_at.is_some() {
                    return Err(DomainError::InvalidValue("session.completed_at"));
                }
            }
            SessionStatus::Completed | SessionStatus::Abandoned => {
                if self
                    .completed_at
                    .is_none_or(|completed| completed < self.started_at)
                {
                    return Err(DomainError::InvalidValue("session.completed_at"));
                }
            }
            SessionStatus::Deleted => {}
        }
        if self.crisis_related {
            return Err(DomainError::SafetyBlocked);
        }
        self.source.validate()?;
        if self.source.actor != SourceActor::User {
            return Err(DomainError::InvalidValue("session.source"));
        }
        Ok(())
    }
}

/// Role of a durable transcript message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// Direct user text, eligible as quoted evidence when policy allows.
    User,
    /// Model response retained as transcript, not accepted as identity evidence by default.
    Assistant,
}

/// One durable, provenance-preserving session message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMessage {
    /// Public schema version.
    pub schema_version: String,
    /// Message identifier.
    pub id: EntityId,
    /// Owning session.
    pub session_id: EntityId,
    /// Transcript role.
    pub role: MessageRole,
    /// Bounded user-visible content.
    pub content: String,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Message sensitivity.
    pub sensitivity: Sensitivity,
    /// Acute-crisis messages are not ordinary identity memory.
    pub crisis_related: bool,
    /// Direct-user or provider provenance.
    pub source: Source,
}

impl SessionMessage {
    /// Construct direct user transcript content.
    #[must_use]
    pub fn user(
        session_id: EntityId,
        content: impl Into<String>,
        sensitivity: Sensitivity,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Message),
            session_id,
            role: MessageRole::User,
            content: content.into(),
            created_at,
            sensitivity,
            crisis_related: false,
            source: Source::user(),
        }
    }

    /// Construct a locally validated provider response.
    #[must_use]
    pub fn assistant(
        session_id: EntityId,
        content: impl Into<String>,
        sensitivity: Sensitivity,
        source: Source,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Message),
            session_id,
            role: MessageRole::Assistant,
            content: content.into(),
            created_at,
            sensitivity,
            crisis_related: false,
            source,
        }
    }
}

impl Validate for SessionMessage {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("message.schema_version"));
        }
        self.id.require_kind(EntityKind::Message)?;
        self.session_id.require_kind(EntityKind::Session)?;
        validate_text(&self.content, 120_000, "message.content")?;
        if self.crisis_related {
            return Err(DomainError::SafetyBlocked);
        }
        self.source.validate()?;
        let valid_source = matches!(
            (self.role, self.source.actor),
            (MessageRole::User, SourceActor::User) | (MessageRole::Assistant, SourceActor::Model)
        );
        if !valid_source {
            return Err(DomainError::InvalidValue("message.source"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_user_can_close_an_active_session() -> Result<(), Box<dyn std::error::Error>> {
        let now = Utc::now();
        let mut session = ReflectionSession::user_started(
            "free-reflection@1.0.0",
            None,
            Sensitivity::Private,
            now,
        );
        assert!(session.close(Actor::model_host(), true, now).is_err());
        session.close(Actor::user(), true, now)?;
        assert_eq!(session.status, SessionStatus::Completed);
        Ok(())
    }
}
