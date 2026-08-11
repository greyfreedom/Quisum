use std::{collections::BTreeSet, net::IpAddr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::{DomainError, EntityId, EntityKind, Sensitivity, Validate, validation::validate_text};

/// Wire protocol used by a model-provider profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI Responses API.
    Openai,
    /// Anthropic Messages API.
    Anthropic,
    /// A remote API implementing OpenAI Chat Completions.
    OpenaiCompatible,
    /// A loopback-only OpenAI-compatible HTTP endpoint.
    LocalHttp,
}

/// Whether a provider endpoint crosses the local-device trust boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyKind {
    /// A remote service receives the authorized context packet.
    Cloud,
    /// A loopback endpoint processes data on this device.
    Local,
}

/// Declared provider features. These are policy-visible claims, not guarantees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Whether the endpoint supports server-sent streaming.
    pub streaming: bool,
    /// Whether the endpoint supports model tool calls.
    pub tool_use: bool,
    /// Whether strict JSON Schema output is supported.
    pub json_schema: bool,
    /// Whether image inputs are supported.
    pub vision: bool,
    /// Declared context window, when known.
    pub context_limit: Option<u32>,
}

/// Non-secret, user-visible model provider configuration.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProfile {
    /// Public schema version.
    pub schema_version: String,
    /// Provider identifier.
    pub id: EntityId,
    /// User-facing name.
    pub name: String,
    /// Provider wire protocol.
    pub kind: ProviderKind,
    /// Endpoint root. Credentials are forbidden in this URL.
    pub base_url: String,
    /// Provider model identifier.
    pub model: String,
    /// Local or cloud privacy boundary.
    pub privacy_kind: PrivacyKind,
    /// Declared protocol features.
    pub capabilities: ProviderCapabilities,
    /// Sensitivity classes the user allows this profile to receive.
    pub trusted_sensitivities: BTreeSet<Sensitivity>,
    /// Opaque lookup name for a separately stored API credential.
    pub credential_ref: Option<String>,
    /// User- or adapter-supplied provider retention note.
    pub retention_note: Option<String>,
    /// Whether new calls may use this profile.
    pub enabled: bool,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last settings change.
    pub updated_at: DateTime<Utc>,
}

impl std::fmt::Debug for ProviderProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderProfile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("privacy_kind", &self.privacy_kind)
            .field("capabilities", &self.capabilities)
            .field("trusted_sensitivities", &self.trusted_sensitivities)
            .field(
                "credential_ref",
                &self.credential_ref.as_ref().map(|_| "[SET]"),
            )
            .field("retention_note", &self.retention_note)
            .field("enabled", &self.enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl ProviderProfile {
    /// Construct a profile. Call [`Validate::validate`] before persistence or use.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        kind: ProviderKind,
        base_url: impl Into<String>,
        model: impl Into<String>,
        privacy_kind: PrivacyKind,
        capabilities: ProviderCapabilities,
        trusted_sensitivities: BTreeSet<Sensitivity>,
        credential_ref: Option<String>,
        retention_note: Option<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: "1.0.0".to_owned(),
            id: EntityId::new(EntityKind::Provider),
            name: name.into(),
            kind,
            base_url: base_url.into(),
            model: model.into(),
            privacy_kind,
            capabilities,
            trusted_sensitivities,
            credential_ref,
            retention_note,
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Return a validated endpoint URL with a provider-relative suffix appended.
    pub fn endpoint(&self, suffix: &str) -> Result<Url, DomainError> {
        self.validate()?;
        if suffix.is_empty() || !suffix.starts_with('/') || suffix.contains(['?', '#']) {
            return Err(DomainError::InvalidValue("provider.endpoint_suffix"));
        }
        let endpoint = format!("{}{}", self.base_url.trim_end_matches('/'), suffix);
        Url::parse(&endpoint).map_err(|_| DomainError::InvalidValue("provider.base_url"))
    }

    /// Check whether this profile may receive one sensitivity class.
    #[must_use]
    pub fn trusts(&self, sensitivity: Sensitivity) -> bool {
        sensitivity.provider_eligible() && self.trusted_sensitivities.contains(&sensitivity)
    }
}

impl Validate for ProviderProfile {
    fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != "1.0.0" {
            return Err(DomainError::InvalidValue("provider.schema_version"));
        }
        self.id.require_kind(EntityKind::Provider)?;
        validate_text(&self.name, 120, "provider.name")?;
        validate_text(&self.model, 160, "provider.model")?;
        if self.base_url.chars().count() > 1_000 {
            return Err(DomainError::InvalidValue("provider.base_url"));
        }
        let url = Url::parse(&self.base_url)
            .map_err(|_| DomainError::InvalidValue("provider.base_url"))?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
        {
            return Err(DomainError::InvalidValue("provider.base_url"));
        }
        let loopback = url.host().is_some_and(host_is_loopback);
        match self.privacy_kind {
            PrivacyKind::Cloud if url.scheme() != "https" || loopback => {
                return Err(DomainError::InvalidValue("provider.cloud_endpoint"));
            }
            PrivacyKind::Local if !loopback => {
                return Err(DomainError::InvalidValue("provider.local_endpoint"));
            }
            PrivacyKind::Cloud | PrivacyKind::Local => {}
        }
        match self.kind {
            ProviderKind::Openai | ProviderKind::Anthropic
                if self.privacy_kind != PrivacyKind::Cloud =>
            {
                return Err(DomainError::InvalidValue("provider.privacy_kind"));
            }
            ProviderKind::LocalHttp if self.privacy_kind != PrivacyKind::Local => {
                return Err(DomainError::InvalidValue("provider.privacy_kind"));
            }
            ProviderKind::Openai
            | ProviderKind::Anthropic
            | ProviderKind::OpenaiCompatible
            | ProviderKind::LocalHttp => {}
        }
        if self
            .capabilities
            .context_limit
            .is_some_and(|limit| limit == 0)
        {
            return Err(DomainError::InvalidValue("provider.context_limit"));
        }
        if self.trusted_sensitivities.is_empty()
            || self.trusted_sensitivities.len() > 3
            || self.trusted_sensitivities.contains(&Sensitivity::LocalOnly)
        {
            return Err(DomainError::InvalidValue("provider.trusted_sensitivities"));
        }
        if let Some(reference) = &self.credential_ref {
            validate_credential_ref(reference)?;
        }
        if self.privacy_kind == PrivacyKind::Cloud && self.credential_ref.is_none() {
            return Err(DomainError::InvalidValue("provider.credential_ref"));
        }
        if let Some(note) = &self.retention_note {
            if note.chars().count() > 2_000 {
                return Err(DomainError::InvalidValue("provider.retention_note"));
            }
        }
        if self.updated_at < self.created_at {
            return Err(DomainError::InvalidValue("provider.updated_at"));
        }
        Ok(())
    }
}

fn host_is_loopback(host: Host<&str>) -> bool {
    match host {
        Host::Domain(name) => {
            name.eq_ignore_ascii_case("localhost")
                || name.to_ascii_lowercase().ends_with(".localhost")
        }
        Host::Ipv4(address) => IpAddr::V4(address).is_loopback(),
        Host::Ipv6(address) => IpAddr::V6(address).is_loopback(),
    }
}

fn validate_credential_ref(reference: &str) -> Result<(), DomainError> {
    let mut bytes = reference.bytes();
    let valid = reference.len() <= 160
        && bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(DomainError::InvalidValue("provider.credential_ref"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tool_use: false,
            json_schema: true,
            vision: false,
            context_limit: Some(128_000),
        }
    }

    #[test]
    fn cloud_profiles_require_https_and_an_external_host() {
        let profile = ProviderProfile::new(
            "Unsafe cloud",
            ProviderKind::OpenaiCompatible,
            "http://localhost:8080/v1",
            "synthetic-model",
            PrivacyKind::Cloud,
            capabilities(),
            BTreeSet::from([Sensitivity::Normal]),
            Some("provider.synthetic".to_owned()),
            None,
            Utc::now(),
        );
        assert_eq!(
            profile.validate(),
            Err(DomainError::InvalidValue("provider.cloud_endpoint"))
        );
    }

    #[test]
    fn local_profiles_are_strictly_loopback_and_never_trust_local_only() {
        let mut profile = ProviderProfile::new(
            "Local test",
            ProviderKind::LocalHttp,
            "http://127.0.0.1:11434/v1",
            "synthetic-model",
            PrivacyKind::Local,
            capabilities(),
            BTreeSet::from([Sensitivity::Normal, Sensitivity::Private]),
            None,
            None,
            Utc::now(),
        );
        assert!(profile.validate().is_ok());
        profile.trusted_sensitivities.insert(Sensitivity::LocalOnly);
        assert_eq!(
            profile.validate(),
            Err(DomainError::InvalidValue("provider.trusted_sensitivities"))
        );
    }

    #[test]
    fn endpoint_preserves_a_versioned_base_path() -> Result<(), DomainError> {
        let profile = ProviderProfile::new(
            "OpenAI",
            ProviderKind::Openai,
            "https://api.openai.com/v1/",
            "synthetic-model",
            PrivacyKind::Cloud,
            capabilities(),
            BTreeSet::from([Sensitivity::Normal]),
            Some("provider.openai".to_owned()),
            None,
            Utc::now(),
        );
        assert_eq!(
            profile.endpoint("/responses")?.as_str(),
            "https://api.openai.com/v1/responses"
        );
        Ok(())
    }
}
