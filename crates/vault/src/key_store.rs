use std::path::Path;

use keyring::v1::Entry;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{VaultError, VaultKey};

const VAULT_KEY_SERVICE: &str = "org.quisum.vault-key";
const PROVIDER_SECRET_SERVICE: &str = "org.quisum.provider-secret";

/// Platform credential-store adapter. Secret values never implement serialization or ordinary Debug.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsSecretStore;

impl OsSecretStore {
    /// Store a vault key under an opaque fingerprint of its canonical path.
    pub fn put_vault_key(self, path: &Path, key: &VaultKey) -> Result<(), VaultError> {
        let reference = vault_path_reference(path)?;
        entry(VAULT_KEY_SERVICE, &reference)?
            .set_secret(key.bytes())
            .map_err(|_| VaultError::SecretStoreUnavailable)
    }

    /// Load a vault key associated with its canonical path.
    pub fn get_vault_key(self, path: &Path) -> Result<VaultKey, VaultError> {
        let reference = vault_path_reference(path)?;
        let secret = Zeroizing::new(
            entry(VAULT_KEY_SERVICE, &reference)?
                .get_secret()
                .map_err(map_lookup_error)?,
        );
        let bytes: [u8; VaultKey::LENGTH] = secret
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::SecretStoreUnavailable)?;
        Ok(VaultKey::from_bytes(bytes))
    }

    /// Remove a vault-key credential after the caller has confirmed the destructive action.
    pub fn delete_vault_key(self, path: &Path) -> Result<(), VaultError> {
        let reference = vault_path_reference(path)?;
        entry(VAULT_KEY_SERVICE, &reference)?
            .delete_credential()
            .map_err(map_lookup_error)
    }

    /// Store an API credential under a schema-validated opaque reference.
    pub fn put_provider_secret(self, reference: &str, secret: &[u8]) -> Result<(), VaultError> {
        if secret.is_empty() || secret.len() > 16 * 1_024 || secret.contains(&0) {
            return Err(VaultError::SecretStoreUnavailable);
        }
        let username = provider_reference(reference)?;
        entry(PROVIDER_SECRET_SERVICE, &username)?
            .set_secret(secret)
            .map_err(|_| VaultError::SecretStoreUnavailable)
    }

    /// Load an API credential into zeroizing memory.
    pub fn get_provider_secret(self, reference: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        let username = provider_reference(reference)?;
        entry(PROVIDER_SECRET_SERVICE, &username)?
            .get_secret()
            .map(Zeroizing::new)
            .map_err(map_lookup_error)
    }

    /// Delete an API credential without changing its non-secret provider profile.
    pub fn delete_provider_secret(self, reference: &str) -> Result<(), VaultError> {
        let username = provider_reference(reference)?;
        entry(PROVIDER_SECRET_SERVICE, &username)?
            .delete_credential()
            .map_err(map_lookup_error)
    }
}

fn entry(service: &str, username: &str) -> Result<Entry, VaultError> {
    Entry::new(service, username).map_err(|_| VaultError::SecretStoreUnavailable)
}

fn vault_path_reference(path: &Path) -> Result<String, VaultError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| VaultError::FileSystem {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(hex::encode(Sha256::digest(
        canonical.as_os_str().as_encoded_bytes(),
    )))
}

fn provider_reference(reference: &str) -> Result<String, VaultError> {
    let mut bytes = reference.bytes();
    let valid = reference.len() <= 160
        && bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        });
    if !valid {
        return Err(VaultError::SecretStoreUnavailable);
    }
    Ok(hex::encode(Sha256::digest(reference.as_bytes())))
}

fn map_lookup_error(error: keyring::Error) -> VaultError {
    match error {
        keyring::Error::NoEntry => VaultError::SecretNotFound,
        _ => VaultError::SecretStoreUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_provider_secrets_are_rejected_before_keyring_access() {
        for secret in [Vec::new(), vec![0], vec![b'x'; 16 * 1_024 + 1]] {
            assert!(
                OsSecretStore
                    .put_provider_secret("provider.synthetic", &secret)
                    .is_err()
            );
        }
    }
}
