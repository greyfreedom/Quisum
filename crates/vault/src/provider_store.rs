use chrono::{DateTime, Utc};
use quisum_core::{
    Actor, ActorKind, AuditAction, AuditDetails, AuditEvent, Capability, EntityId, EntityKind,
    ProviderProfile, Validate,
};
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{SqlCipherVault, VaultError, store};

impl SqlCipherVault {
    /// Insert or update non-secret provider metadata through a user-authorized settings surface.
    pub fn put_provider_profile(
        &self,
        actor: Actor,
        profile: &ProviderProfile,
    ) -> Result<(), VaultError> {
        actor.require(Capability::ChangePrivacyPolicy)?;
        if actor.kind() != ActorKind::User {
            return Err(
                quisum_core::DomainError::Forbidden(Capability::ChangePrivacyPolicy).into(),
            );
        }
        profile.validate()?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT payload_json FROM provider_profiles WHERE id = ?1",
                [profile.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str::<ProviderProfile>(&payload))
            .transpose()?;
        if let Some(existing) = existing {
            existing.validate()?;
            if existing.name != profile.name
                || existing.kind != profile.kind
                || existing.base_url != profile.base_url
                || existing.model != profile.model
                || existing.privacy_kind != profile.privacy_kind
                || existing.credential_ref != profile.credential_ref
                || existing.created_at != profile.created_at
                || profile.updated_at <= existing.updated_at
            {
                return Err(
                    quisum_core::DomainError::InvalidValue("provider.immutable_identity").into(),
                );
            }
        }
        transaction
            .execute(
                "INSERT INTO provider_profiles(
                   id, kind, privacy_kind, enabled, created_at, updated_at, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                   kind = excluded.kind,
                   privacy_kind = excluded.privacy_kind,
                   enabled = excluded.enabled,
                   updated_at = excluded.updated_at,
                   payload_json = excluded.payload_json",
                params![
                    profile.id.to_string(),
                    store::enum_token(&profile.kind)?,
                    store::enum_token(&profile.privacy_kind)?,
                    i64::from(profile.enabled),
                    store::format_time(profile.created_at),
                    store::format_time(profile.updated_at),
                    serde_json::to_string(profile)?
                ],
            )
            .map_err(store::map_constraint)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::PolicyChanged,
            Some(profile.id.clone()),
            AuditDetails::None,
            profile.updated_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }

    /// Fetch one provider profile without exposing any credential material.
    pub fn get_provider_profile(
        &self,
        actor: Actor,
        profile_id: &EntityId,
    ) -> Result<Option<ProviderProfile>, VaultError> {
        actor.require(Capability::ReadBoundedRecord)?;
        profile_id.require_kind(EntityKind::Provider)?;
        let connection = self.connection.lock();
        let payload = connection
            .query_row(
                "SELECT payload_json FROM provider_profiles WHERE id = ?1",
                [profile_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        payload
            .map(|payload| {
                let profile: ProviderProfile = serde_json::from_str(&payload)?;
                profile.validate()?;
                Ok(profile)
            })
            .transpose()
    }

    /// List provider metadata for an authorized local settings surface.
    pub fn list_provider_profiles(&self, actor: Actor) -> Result<Vec<ProviderProfile>, VaultError> {
        actor.require(Capability::ChangePrivacyPolicy)?;
        if actor.kind() != ActorKind::User {
            return Err(
                quisum_core::DomainError::Forbidden(Capability::ChangePrivacyPolicy).into(),
            );
        }
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare("SELECT payload_json FROM provider_profiles ORDER BY updated_at DESC, id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut profiles = Vec::new();
        for row in rows {
            let profile: ProviderProfile = serde_json::from_str(&row?)?;
            profile.validate()?;
            profiles.push(profile);
        }
        Ok(profiles)
    }

    /// Append a content-free provider connection-test result.
    pub fn record_provider_test(
        &self,
        actor: Actor,
        profile_id: &EntityId,
        succeeded: bool,
        tested_at: DateTime<Utc>,
    ) -> Result<(), VaultError> {
        actor.require(Capability::ChangePrivacyPolicy)?;
        if actor.kind() != ActorKind::User {
            return Err(
                quisum_core::DomainError::Forbidden(Capability::ChangePrivacyPolicy).into(),
            );
        }
        profile_id.require_kind(EntityKind::Provider)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_profiles WHERE id = ?1)",
            [profile_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(VaultError::NotFoundRecord);
        }
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::ProviderTested,
            Some(profile_id.clone()),
            AuditDetails::ProviderTest { succeeded },
            tested_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, error::Error};

    use quisum_core::{PrivacyKind, ProviderCapabilities, ProviderKind, Sensitivity};
    use tempfile::tempdir;

    use super::*;
    use crate::VaultKey;

    #[test]
    fn provider_metadata_round_trips_without_a_secret() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let profile = ProviderProfile::new(
            "Synthetic local",
            ProviderKind::LocalHttp,
            "http://127.0.0.1:11434/v1",
            "synthetic-model",
            PrivacyKind::Local,
            ProviderCapabilities {
                streaming: true,
                tool_use: false,
                json_schema: true,
                vision: false,
                context_limit: Some(8_192),
            },
            BTreeSet::from([Sensitivity::Normal, Sensitivity::Private]),
            None,
            Some("Synthetic endpoint; no retention claim.".to_owned()),
            Utc::now(),
        );
        vault.put_provider_profile(Actor::user(), &profile)?;
        assert_eq!(
            vault.get_provider_profile(Actor::user(), &profile.id)?,
            Some(profile.clone())
        );
        assert_eq!(vault.list_provider_profiles(Actor::user())?, vec![profile]);
        Ok(())
    }

    #[test]
    fn model_host_cannot_change_provider_settings() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let profile = ProviderProfile::new(
            "Synthetic local",
            ProviderKind::LocalHttp,
            "http://localhost:11434/v1",
            "synthetic-model",
            PrivacyKind::Local,
            ProviderCapabilities {
                streaming: false,
                tool_use: false,
                json_schema: false,
                vision: false,
                context_limit: None,
            },
            BTreeSet::from([Sensitivity::Normal]),
            None,
            None,
            Utc::now(),
        );
        assert!(matches!(
            vault.put_provider_profile(Actor::model_host(), &profile),
            Err(VaultError::Domain(quisum_core::DomainError::Forbidden(_)))
        ));
        Ok(())
    }

    #[test]
    fn an_existing_provider_identity_cannot_be_rewritten() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let mut profile = ProviderProfile::new(
            "Synthetic stable profile",
            ProviderKind::LocalHttp,
            "http://127.0.0.1:11434/v1",
            "synthetic-model",
            PrivacyKind::Local,
            ProviderCapabilities {
                streaming: false,
                tool_use: false,
                json_schema: false,
                vision: false,
                context_limit: None,
            },
            BTreeSet::from([Sensitivity::Normal]),
            None,
            None,
            Utc::now(),
        );
        vault.put_provider_profile(Actor::user(), &profile)?;
        profile.name = "Forged replacement".to_owned();
        profile.updated_at += chrono::Duration::seconds(1);
        assert!(vault.put_provider_profile(Actor::user(), &profile).is_err());
        Ok(())
    }
}
