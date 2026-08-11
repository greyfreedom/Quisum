use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Actor, ActorKind, Capability, DomainError, EntityId, EntityKind, Sensitivity, Source, Validate,
    validation::validate_text,
};

/// Risk class permitted for a reflective behavioral experiment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentRisk {
    /// Low-consequence and readily reversible.
    Low,
    /// Requires explicit human review because consequences are less trivial.
    Moderate,
}

/// Behavioral experiment lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    /// Proposed for review.
    Proposed,
    /// Explicitly activated by the user.
    Active,
    /// Completed with an observed result.
    Completed,
    /// Cancelled without penalty.
    Cancelled,
    /// Deleted.
    Deleted,
}

/// A bounded, voluntary real-world test of a self-hypothesis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehavioralExperiment {
    /// Public schema version.
    pub schema_version: String,
    /// Experiment identifier.
    pub id: EntityId,
    /// Short title.
    pub title: String,
    /// Hypothesis being tested.
    pub hypothesis: String,
    /// Observable prediction recorded before action.
    pub prediction: String,
    /// Bounded action.
    pub action: String,
    /// Condition that ends or cancels the experiment.
    pub stop_condition: String,
    /// Risk class.
    pub risk_level: ExperimentRisk,
    /// Lifecycle state.
    pub status: ExperimentStatus,
    /// Related claims.
    pub related_claims: BTreeSet<EntityId>,
    /// Planned time.
    pub planned_at: DateTime<Utc>,
    /// Completion time.
    pub completed_at: Option<DateTime<Utc>>,
    /// Expected result recorded in advance.
    pub expected_result: Option<String>,
    /// Actual observation.
    pub actual_result: Option<String>,
    /// User's learning or claim update.
    pub learning: Option<String>,
    /// Sensitivity policy.
    pub sensitivity: Sensitivity,
    /// Crisis exclusion flag.
    pub crisis_related: bool,
    /// Provenance.
    pub source: Source,
}

impl BehavioralExperiment {
    /// Create a proposal. Activation always remains a human operation.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn proposal(
        title: impl Into<String>,
        hypothesis: impl Into<String>,
        prediction: impl Into<String>,
        action: impl Into<String>,
        stop_condition: impl Into<String>,
        risk_level: ExperimentRisk,
        planned_at: DateTime<Utc>,
        sensitivity: Sensitivity,
        source: Source,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Experiment),
            title: title.into(),
            hypothesis: hypothesis.into(),
            prediction: prediction.into(),
            action: action.into(),
            stop_condition: stop_condition.into(),
            risk_level,
            status: ExperimentStatus::Proposed,
            related_claims: BTreeSet::new(),
            planned_at,
            completed_at: None,
            expected_result: None,
            actual_result: None,
            learning: None,
            sensitivity,
            crisis_related: false,
            source,
        }
    }

    /// Activate a reviewed experiment.
    pub fn activate(&mut self, actor: Actor) -> Result<(), DomainError> {
        actor.require(Capability::ReviewExperiment)?;
        if actor.kind() != ActorKind::User || self.status != ExperimentStatus::Proposed {
            return Err(DomainError::Forbidden(Capability::ReviewExperiment));
        }
        self.validate()?;
        let mut updated = self.clone();
        updated.status = ExperimentStatus::Active;
        updated.validate()?;
        *self = updated;
        Ok(())
    }

    /// Complete an active experiment with observed results.
    pub fn complete(
        &mut self,
        actor: Actor,
        actual_result: impl Into<String>,
        learning: impl Into<String>,
        completed_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        actor.require(Capability::ReviewExperiment)?;
        if actor.kind() != ActorKind::User || self.status != ExperimentStatus::Active {
            return Err(DomainError::Forbidden(Capability::ReviewExperiment));
        }
        self.validate()?;
        let mut updated = self.clone();
        updated.status = ExperimentStatus::Completed;
        updated.actual_result = Some(actual_result.into());
        updated.learning = Some(learning.into());
        updated.completed_at = Some(completed_at);
        updated.validate()?;
        *self = updated;
        Ok(())
    }

    /// Cancel a proposed or active experiment without penalty.
    pub fn cancel(&mut self, actor: Actor) -> Result<(), DomainError> {
        actor.require(Capability::ReviewExperiment)?;
        if actor.kind() != ActorKind::User
            || !matches!(
                self.status,
                ExperimentStatus::Proposed | ExperimentStatus::Active
            )
        {
            return Err(DomainError::Forbidden(Capability::ReviewExperiment));
        }
        self.validate()?;
        let mut updated = self.clone();
        updated.status = ExperimentStatus::Cancelled;
        updated.validate()?;
        *self = updated;
        Ok(())
    }
}

impl Validate for BehavioralExperiment {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("experiment.schema_version"));
        }
        self.id.require_kind(EntityKind::Experiment)?;
        validate_text(&self.title, 240, "experiment.title")?;
        validate_text(&self.hypothesis, 4_000, "experiment.hypothesis")?;
        validate_text(&self.prediction, 4_000, "experiment.prediction")?;
        validate_text(&self.action, 4_000, "experiment.action")?;
        validate_text(&self.stop_condition, 1_000, "experiment.stop_condition")?;
        if self.related_claims.len() > 32 {
            return Err(DomainError::InvalidValue("experiment.related_claims"));
        }
        for claim in &self.related_claims {
            claim.require_kind(EntityKind::Claim)?;
        }
        if let Some(expected) = &self.expected_result {
            validate_text(expected, 4_000, "experiment.expected_result")?;
        }
        match self.status {
            ExperimentStatus::Completed => {
                validate_text(
                    self.actual_result
                        .as_deref()
                        .ok_or(DomainError::InvalidValue("experiment.actual_result"))?,
                    4_000,
                    "experiment.actual_result",
                )?;
                validate_text(
                    self.learning
                        .as_deref()
                        .ok_or(DomainError::InvalidValue("experiment.learning"))?,
                    4_000,
                    "experiment.learning",
                )?;
                if self.completed_at.is_none() {
                    return Err(DomainError::InvalidValue("experiment.completed_at"));
                }
                if self
                    .completed_at
                    .is_some_and(|completed| completed < self.planned_at)
                {
                    return Err(DomainError::InvalidValue("experiment.completed_at"));
                }
            }
            ExperimentStatus::Proposed
            | ExperimentStatus::Active
            | ExperimentStatus::Cancelled
            | ExperimentStatus::Deleted => {
                if self.completed_at.is_some()
                    || self.actual_result.is_some()
                    || self.learning.is_some()
                {
                    return Err(DomainError::InvalidValue("experiment.completion_fields"));
                }
            }
        }
        if self.crisis_related {
            return Err(DomainError::SafetyBlocked);
        }
        self.source.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> BehavioralExperiment {
        BehavioralExperiment::proposal(
            "Synthetic experiment",
            "A contextual hypothesis.",
            "One observable prediction.",
            "One reversible action.",
            "Stop if consequences become unclear.",
            ExperimentRisk::Low,
            Utc::now(),
            Sensitivity::Private,
            Source::user(),
        )
    }

    #[test]
    fn failed_completion_leaves_the_active_projection_unchanged() {
        let mut experiment = proposal();
        assert!(experiment.activate(Actor::user()).is_ok());
        let before = experiment.clone();
        assert!(
            experiment
                .complete(Actor::user(), "", "Synthetic learning", Utc::now())
                .is_err()
        );
        assert_eq!(experiment, before);
    }

    #[test]
    fn model_host_cannot_activate_or_cancel_an_experiment() {
        let mut experiment = proposal();
        assert!(experiment.activate(Actor::model_host()).is_err());
        assert!(experiment.cancel(Actor::model_host()).is_err());
        assert_eq!(experiment.status, ExperimentStatus::Proposed);
    }
}
