use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    DomainError, EntityId, EntityKind, Sensitivity, Source, Validate,
    validation::{validate_string_list, validate_text, validate_topic},
};

/// Lifecycle state for a concrete event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    /// Proposed by a model or import and awaiting user review.
    Proposed,
    /// Accepted or directly captured by the user.
    Confirmed,
    /// Rejected by the user and ineligible for retrieval.
    Rejected,
    /// Deleted and unavailable as evidence.
    Deleted,
}

/// A concrete occurrence kept separate from interpretations about identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Public schema version.
    pub schema_version: String,
    /// Event identifier.
    pub id: EntityId,
    /// Short user-facing title.
    pub title: String,
    /// Review state.
    pub status: EventStatus,
    /// When the occurrence happened, if known.
    pub occurred_at: Option<DateTime<Utc>>,
    /// When it was recorded.
    pub recorded_at: DateTime<Utc>,
    /// Externally observable or directly reported facts.
    pub facts: Vec<String>,
    /// The user's interpretations, kept distinct from facts.
    pub interpretations: Vec<String>,
    /// User-described emotions.
    pub emotions: Vec<String>,
    /// User-described bodily responses.
    pub bodily_responses: Vec<String>,
    /// Actions taken.
    pub actions: Vec<String>,
    /// Immediate outcomes.
    pub short_term_outcomes: Vec<String>,
    /// Later outcomes.
    pub long_term_outcomes: Vec<String>,
    /// Retrieval topics.
    pub topics: BTreeSet<String>,
    /// Sensitivity policy.
    pub sensitivity: Sensitivity,
    /// Whether the event originated in an acute-crisis route.
    pub crisis_related: bool,
    /// Provenance.
    pub source: Source,
}

impl Event {
    /// Create a user-authored confirmed event.
    #[must_use]
    pub fn user_confirmed(
        title: impl Into<String>,
        facts: Vec<String>,
        occurred_at: Option<DateTime<Utc>>,
        recorded_at: DateTime<Utc>,
        sensitivity: Sensitivity,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Event),
            title: title.into(),
            status: EventStatus::Confirmed,
            occurred_at,
            recorded_at,
            facts,
            interpretations: Vec::new(),
            emotions: Vec::new(),
            bodily_responses: Vec::new(),
            actions: Vec::new(),
            short_term_outcomes: Vec::new(),
            long_term_outcomes: Vec::new(),
            topics: BTreeSet::new(),
            sensitivity,
            crisis_related: false,
            source: Source::user(),
        }
    }

    /// Create a model-proposed event that requires human review.
    #[must_use]
    pub fn proposed(
        title: impl Into<String>,
        facts: Vec<String>,
        recorded_at: DateTime<Utc>,
        sensitivity: Sensitivity,
        source: Source,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Event),
            title: title.into(),
            status: EventStatus::Proposed,
            occurred_at: None,
            recorded_at,
            facts,
            interpretations: Vec::new(),
            emotions: Vec::new(),
            bodily_responses: Vec::new(),
            actions: Vec::new(),
            short_term_outcomes: Vec::new(),
            long_term_outcomes: Vec::new(),
            topics: BTreeSet::new(),
            sensitivity,
            crisis_related: false,
            source,
        }
    }
}

impl Validate for Event {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("event.schema_version"));
        }
        self.id.require_kind(EntityKind::Event)?;
        validate_text(&self.title, 240, "event.title")?;
        if self.facts.is_empty() {
            return Err(DomainError::InvalidValue("event.facts"));
        }
        validate_string_list(&self.facts, 64, 4_000, "event.facts")?;
        validate_string_list(&self.interpretations, 32, 4_000, "event.interpretations")?;
        if self.emotions.len() > 32 {
            return Err(DomainError::InvalidValue("event.emotions"));
        }
        for emotion in &self.emotions {
            validate_topic(emotion, "event.emotions")?;
        }
        validate_string_list(&self.bodily_responses, 32, 4_000, "event.bodily_responses")?;
        validate_string_list(&self.actions, 32, 4_000, "event.actions")?;
        validate_string_list(
            &self.short_term_outcomes,
            32,
            4_000,
            "event.short_term_outcomes",
        )?;
        validate_string_list(
            &self.long_term_outcomes,
            32,
            4_000,
            "event.long_term_outcomes",
        )?;
        if self.topics.len() > 32 {
            return Err(DomainError::InvalidValue("event.topics"));
        }
        for topic in &self.topics {
            validate_topic(topic, "event.topics")?;
        }
        if self
            .occurred_at
            .is_some_and(|occurred| occurred > self.recorded_at)
        {
            return Err(DomainError::InvalidValue("event.occurred_at"));
        }
        self.source.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_requires_at_least_one_fact() {
        let event = Event::user_confirmed(
            "Synthetic event",
            Vec::new(),
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        assert_eq!(
            event.validate(),
            Err(DomainError::InvalidValue("event.facts"))
        );
    }
}
