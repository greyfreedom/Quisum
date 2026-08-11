use std::collections::BTreeSet;

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

const EMBEDDED_RESOURCES: &str = include_str!("../../../resources/crisis/v1.0.0.json");
const MAX_MESSAGE_CHARACTERS: usize = 2_000;
const MAX_LIMITATIONS_CHARACTERS: usize = 2_000;
const MAX_SOURCES: usize = 8;
const MAX_REGIONS: usize = 64;

/// A malformed or internally inconsistent bundled crisis resource pack.
#[derive(Debug, thiserror::Error)]
pub enum CrisisResourceError {
    /// The embedded JSON did not match the strict resource format.
    #[error("the bundled crisis resource pack is invalid")]
    InvalidSerialization,
    /// Version, date, region, content, or source invariants failed.
    #[error("the bundled crisis resource pack failed validation")]
    InvalidPolicy,
}

/// One region's bounded, source-linked immediate-support guidance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrisisRegionResource {
    pub region: String,
    pub aliases: Vec<String>,
    pub locale: String,
    pub immediate_danger: String,
    pub additional_support: Option<String>,
    pub source_urls: Vec<String>,
}

/// Versioned local data used only after deterministic crisis routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrisisResourcePack {
    #[serde(rename = "$schema")]
    pub schema_uri: String,
    pub schema_version: String,
    pub pack_version: String,
    pub source_checked_at: NaiveDate,
    pub source_check_due: NaiveDate,
    pub limitations: String,
    pub fallback: CrisisRegionResource,
    pub regions: Vec<CrisisRegionResource>,
}

/// Display-ready crisis guidance with provenance and freshness metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrisisGuidance {
    pub pack_version: String,
    pub region: String,
    pub source_checked_at: NaiveDate,
    pub source_check_due: NaiveDate,
    pub is_current: bool,
    pub assistant_response: String,
    pub source_urls: Vec<String>,
    pub limitations: String,
}

impl CrisisResourcePack {
    /// Parse and validate the resource pack compiled into every Core consumer.
    pub fn embedded() -> Result<Self, CrisisResourceError> {
        let pack: Self = serde_json::from_str(EMBEDDED_RESOURCES)
            .map_err(|_| CrisisResourceError::InvalidSerialization)?;
        pack.validate()?;
        Ok(pack)
    }

    /// Select a locale-derived region or the conservative no-number fallback.
    pub fn guidance(&self, region_or_locale: Option<&str>) -> CrisisGuidance {
        self.guidance_at(region_or_locale, Utc::now().date_naive())
    }

    /// Select guidance as of a supplied date so freshness behavior remains testable.
    #[must_use]
    pub fn guidance_at(
        &self,
        region_or_locale: Option<&str>,
        on_date: NaiveDate,
    ) -> CrisisGuidance {
        let is_current = on_date <= self.source_check_due;
        let requested = region_or_locale.and_then(normalize_region);
        let selected = if is_current {
            requested
                .as_deref()
                .and_then(|region| {
                    self.regions.iter().find(|entry| {
                        entry.region == region || entry.aliases.iter().any(|alias| alias == region)
                    })
                })
                .unwrap_or(&self.fallback)
        } else {
            &self.fallback
        };
        let mut assistant_response = format!(
            "Quisum is not an emergency service. Stop identity analysis. {}",
            selected.immediate_danger
        );
        if let Some(additional) = selected.additional_support.as_deref() {
            assistant_response.push(' ');
            assistant_response.push_str(additional);
        }
        assistant_response.push_str(" Contact a trusted person who can help you reach emergency support without putting themselves in danger. Automated crisis detection and this local resource pack can be incomplete or outdated; verify local availability when possible.");
        CrisisGuidance {
            pack_version: self.pack_version.clone(),
            region: selected.region.clone(),
            source_checked_at: self.source_checked_at,
            source_check_due: self.source_check_due,
            is_current,
            assistant_response,
            source_urls: selected.source_urls.clone(),
            limitations: self.limitations.clone(),
        }
    }

    fn validate(&self) -> Result<(), CrisisResourceError> {
        if self.schema_uri != "../../schemas/crisis-resource-pack.schema.json"
            || self.schema_version != "1.0.0"
            || self.pack_version != "1.0.0"
            || self.source_check_due <= self.source_checked_at
            || self.regions.is_empty()
            || self.regions.len() > MAX_REGIONS
            || !bounded(&self.limitations, MAX_LIMITATIONS_CHARACTERS)
            || self.fallback.region != "DEFAULT"
        {
            return Err(CrisisResourceError::InvalidPolicy);
        }
        let mut identifiers = BTreeSet::new();
        validate_region(&self.fallback, &mut identifiers)?;
        for region in &self.regions {
            validate_region(region, &mut identifiers)?;
        }
        Ok(())
    }
}

fn validate_region(
    resource: &CrisisRegionResource,
    identifiers: &mut BTreeSet<String>,
) -> Result<(), CrisisResourceError> {
    if !valid_region_identifier(&resource.region)
        || !identifiers.insert(resource.region.clone())
        || resource.aliases.len() > 8
        || !bounded(&resource.locale, 32)
        || !bounded(&resource.immediate_danger, MAX_MESSAGE_CHARACTERS)
        || resource
            .additional_support
            .as_deref()
            .is_some_and(|value| !bounded(value, MAX_MESSAGE_CHARACTERS))
        || resource.source_urls.len() > MAX_SOURCES
    {
        return Err(CrisisResourceError::InvalidPolicy);
    }
    for alias in &resource.aliases {
        if !valid_region_identifier(alias) || !identifiers.insert(alias.clone()) {
            return Err(CrisisResourceError::InvalidPolicy);
        }
    }
    for source in &resource.source_urls {
        let parsed = Url::parse(source).map_err(|_| CrisisResourceError::InvalidPolicy)?;
        if parsed.scheme() != "https" || parsed.host_str().is_none() {
            return Err(CrisisResourceError::InvalidPolicy);
        }
    }
    Ok(())
}

fn normalize_region(value: &str) -> Option<String> {
    let normalized = value.trim().replace('_', "-").to_ascii_uppercase();
    if normalized.is_empty() || normalized.len() > 16 {
        return None;
    }
    let candidate = normalized.rsplit('-').next()?;
    valid_region_identifier(candidate).then(|| candidate.to_owned())
}

fn valid_region_identifier(value: &str) -> bool {
    value == "DEFAULT" || (value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_uppercase()))
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= maximum && !value.as_bytes().contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pack_is_valid_and_source_linked() -> Result<(), Box<dyn std::error::Error>> {
        let pack = CrisisResourcePack::embedded()?;
        assert_eq!(pack.pack_version, "1.0.0");
        assert!(pack.regions.len() >= 4);
        assert!(
            pack.regions
                .iter()
                .all(|region| !region.source_urls.is_empty())
        );
        Ok(())
    }

    #[test]
    fn locale_selects_region_and_unsupported_input_falls_back()
    -> Result<(), Box<dyn std::error::Error>> {
        let pack = CrisisResourcePack::embedded()?;
        let us = pack.guidance_at(Some("en-US"), pack.source_checked_at);
        assert_eq!(us.region, "US");
        assert!(us.assistant_response.contains("emergency services"));
        assert!(us.assistant_response.contains("trusted person"));
        assert!(!us.source_urls.is_empty());

        let fallback = pack.guidance_at(Some("unsupported"), pack.source_checked_at);
        assert_eq!(fallback.region, "DEFAULT");
        assert!(fallback.source_urls.is_empty());
        Ok(())
    }

    #[test]
    fn aliases_are_unique_and_uk_maps_to_gb() -> Result<(), Box<dyn std::error::Error>> {
        let pack = CrisisResourcePack::embedded()?;
        assert_eq!(
            pack.guidance_at(Some("UK"), pack.source_checked_at).region,
            "GB"
        );
        Ok(())
    }

    #[test]
    fn overdue_pack_suppresses_region_specific_numbers() -> Result<(), Box<dyn std::error::Error>> {
        let pack = CrisisResourcePack::embedded()?;
        let overdue = pack.guidance_at(
            Some("US"),
            pack.source_check_due + chrono::Duration::days(1),
        );
        assert_eq!(overdue.region, "DEFAULT");
        assert!(!overdue.is_current);
        assert!(overdue.source_urls.is_empty());
        assert!(!overdue.assistant_response.contains("988"));
        Ok(())
    }
}
