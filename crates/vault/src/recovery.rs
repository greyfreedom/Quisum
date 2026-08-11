use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit as _, XChaCha20Poly1305, XNonce,
    aead::{Aead as _, Payload},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{VaultError, VaultKey};

const FORMAT: &str = "quisum-recovery-key";
const VERSION: u32 = 1;
const KDF: &str = "argon2id";
const AEAD: &str = "xchacha20poly1305";
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const KEY_BYTES: usize = 32;
const TAG_BYTES: usize = 16;
const DEFAULT_MEMORY_KIB: u32 = 65_536;
const DEFAULT_ITERATIONS: u32 = 3;
const DEFAULT_PARALLELISM: u32 = 1;

/// Versioned, authenticated wrapping of a random vault key under a recovery passphrase.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryKeyEnvelope {
    /// Stable artifact discriminator.
    pub format: String,
    /// Envelope version.
    pub version: u32,
    /// Password derivation algorithm.
    pub kdf: String,
    /// Argon2 memory cost in KiB.
    pub memory_kib: u32,
    /// Argon2 time cost.
    pub iterations: u32,
    /// Argon2 lane count.
    pub parallelism: u32,
    /// URL-safe base64 salt.
    pub salt: String,
    /// Authenticated-encryption algorithm.
    pub aead: String,
    /// URL-safe base64 nonce.
    pub nonce: String,
    /// URL-safe base64 ciphertext and tag.
    pub ciphertext: String,
}

impl std::fmt::Debug for RecoveryKeyEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryKeyEnvelope")
            .field("format", &self.format)
            .field("version", &self.version)
            .field("kdf", &self.kdf)
            .field("memory_kib", &self.memory_kib)
            .field("iterations", &self.iterations)
            .field("parallelism", &self.parallelism)
            .field("salt", &"[ENCODED SALT]")
            .field("aead", &self.aead)
            .field("nonce", &"[ENCODED NONCE]")
            .field("ciphertext", &"[REDACTED]")
            .finish()
    }
}

impl RecoveryKeyEnvelope {
    /// Encrypt a vault key under an Argon2id-derived key.
    pub fn seal(key: &VaultKey, passphrase: &str) -> Result<Self, VaultError> {
        validate_passphrase(passphrase)?;
        let mut salt = [0_u8; SALT_BYTES];
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut salt).map_err(|_| VaultError::RandomUnavailable)?;
        getrandom::fill(&mut nonce).map_err(|_| VaultError::RandomUnavailable)?;
        let salt_encoded = URL_SAFE_NO_PAD.encode(salt);
        let nonce_encoded = URL_SAFE_NO_PAD.encode(nonce);
        let mut envelope = Self {
            format: FORMAT.to_owned(),
            version: VERSION,
            kdf: KDF.to_owned(),
            memory_kib: DEFAULT_MEMORY_KIB,
            iterations: DEFAULT_ITERATIONS,
            parallelism: DEFAULT_PARALLELISM,
            salt: salt_encoded,
            aead: AEAD.to_owned(),
            nonce: nonce_encoded,
            ciphertext: String::new(),
        };
        let derived = derive_key(&envelope, passphrase, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(derived.as_slice())
            .map_err(|_| VaultError::InvalidRecoveryEnvelope)?;
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: key.bytes(),
                    aad: envelope.associated_data().as_bytes(),
                },
            )
            .map_err(|_| VaultError::RecoveryFailed)?;
        envelope.ciphertext = URL_SAFE_NO_PAD.encode(ciphertext);
        Ok(envelope)
    }

    /// Authenticate and unwrap the stored vault key.
    pub fn open(&self, passphrase: &str) -> Result<VaultKey, VaultError> {
        validate_passphrase(passphrase)?;
        self.validate_shape()?;
        let salt = decode_exact::<SALT_BYTES>(&self.salt)?;
        let nonce = decode_exact::<NONCE_BYTES>(&self.nonce)?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&self.ciphertext)
            .map_err(|_| VaultError::InvalidRecoveryEnvelope)?;
        if ciphertext.len() != KEY_BYTES + TAG_BYTES {
            return Err(VaultError::InvalidRecoveryEnvelope);
        }
        let derived = derive_key(self, passphrase, &salt)?;
        let cipher = XChaCha20Poly1305::new_from_slice(derived.as_slice())
            .map_err(|_| VaultError::InvalidRecoveryEnvelope)?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: self.associated_data().as_bytes(),
                    },
                )
                .map_err(|_| VaultError::RecoveryFailed)?,
        );
        let bytes: [u8; VaultKey::LENGTH] = plaintext
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::InvalidRecoveryEnvelope)?;
        Ok(VaultKey::from_bytes(bytes))
    }

    fn validate_shape(&self) -> Result<(), VaultError> {
        if self.format != FORMAT
            || self.version != VERSION
            || self.kdf != KDF
            || self.aead != AEAD
            || !(32_768..=262_144).contains(&self.memory_kib)
            || !(2..=10).contains(&self.iterations)
            || !(1..=4).contains(&self.parallelism)
            || self.salt.len() > 64
            || self.nonce.len() > 64
            || self.ciphertext.len() > 128
        {
            return Err(VaultError::InvalidRecoveryEnvelope);
        }
        Ok(())
    }

    fn associated_data(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.format,
            self.version,
            self.kdf,
            self.memory_kib,
            self.iterations,
            self.parallelism,
            self.salt,
            self.aead,
            self.nonce
        )
    }
}

fn derive_key(
    envelope: &RecoveryKeyEnvelope,
    passphrase: &str,
    salt: &[u8],
) -> Result<Zeroizing<[u8; KEY_BYTES]>, VaultError> {
    let parameters = Params::new(
        envelope.memory_kib,
        envelope.iterations,
        envelope.parallelism,
        Some(KEY_BYTES),
    )
    .map_err(|_| VaultError::InvalidRecoveryEnvelope)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters);
    let mut output = Zeroizing::new([0_u8; KEY_BYTES]);
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, output.as_mut())
        .map_err(|_| VaultError::RecoveryFailed)?;
    Ok(output)
}

fn decode_exact<const LENGTH: usize>(encoded: &str) -> Result<[u8; LENGTH], VaultError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| VaultError::InvalidRecoveryEnvelope)?;
    decoded
        .try_into()
        .map_err(|_| VaultError::InvalidRecoveryEnvelope)
}

fn validate_passphrase(passphrase: &str) -> Result<(), VaultError> {
    if (12..=1_024).contains(&passphrase.chars().count()) {
        Ok(())
    } else {
        Err(VaultError::RecoveryFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSPHRASE: &str = "synthetic recovery passphrase";

    #[test]
    fn recovery_envelope_round_trips_and_redacts_debug() -> Result<(), VaultError> {
        let key = VaultKey::generate()?;
        let envelope = RecoveryKeyEnvelope::seal(&key, PASSPHRASE)?;
        let recovered = envelope.open(PASSPHRASE)?;
        assert!(key.matches(&recovered));
        assert!(!format!("{envelope:?}").contains(&envelope.ciphertext));
        Ok(())
    }

    #[test]
    fn wrong_passphrase_and_tampering_are_rejected() -> Result<(), VaultError> {
        let key = VaultKey::generate()?;
        let mut envelope = RecoveryKeyEnvelope::seal(&key, PASSPHRASE)?;
        assert!(matches!(
            envelope.open("different synthetic phrase"),
            Err(VaultError::RecoveryFailed)
        ));
        envelope.ciphertext.replace_range(..1, "A");
        assert!(matches!(
            envelope.open(PASSPHRASE),
            Err(VaultError::RecoveryFailed | VaultError::InvalidRecoveryEnvelope)
        ));
        Ok(())
    }

    #[test]
    fn attacker_controlled_kdf_cost_is_bounded_before_derivation() -> Result<(), VaultError> {
        let key = VaultKey::generate()?;
        let mut envelope = RecoveryKeyEnvelope::seal(&key, PASSPHRASE)?;
        envelope.memory_kib = u32::MAX;
        assert!(matches!(
            envelope.open(PASSPHRASE),
            Err(VaultError::InvalidRecoveryEnvelope)
        ));
        Ok(())
    }
}
