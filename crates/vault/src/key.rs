use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::VaultError;

/// A random 256-bit SQLCipher key that is zeroized on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey([u8; Self::LENGTH]);

impl VaultKey {
    /// Required key length in bytes.
    pub const LENGTH: usize = 32;

    /// Generate a key from the operating system's cryptographic RNG.
    pub fn generate() -> Result<Self, VaultError> {
        let mut bytes = [0_u8; Self::LENGTH];
        getrandom::fill(&mut bytes).map_err(|_| VaultError::RandomUnavailable)?;
        Ok(Self(bytes))
    }

    /// Reconstruct a key obtained from an approved secret store.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    pub(crate) const fn duplicate(&self) -> Self {
        Self(self.0)
    }

    pub(crate) const fn bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        let mut difference = 0_u8;
        for (left, right) in self.0.iter().zip(other.0.iter()) {
            difference |= left ^ right;
        }
        difference == 0
    }

    pub(crate) fn sqlcipher_literal(&self) -> Zeroizing<String> {
        Zeroizing::new(format!("x'{}'", hex::encode(self.0.as_slice())))
    }
}

impl fmt::Debug for VaultKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VaultKey([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_contains_key_material() {
        let key = VaultKey::from_bytes([0xAB; VaultKey::LENGTH]);
        let debug = format!("{key:?}");
        assert_eq!(debug, "VaultKey([REDACTED])");
        assert!(!debug.contains("ab"));
    }
}
