//! Network addresses derived from public keys.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::crypto::{hash_data, PublicKey};

/// A network address derived from a public key.
///
/// Address = BLAKE3(PublicKey)[0..20] (20 bytes, similar to Ethereum)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address([u8; 20]);

impl Address {
    /// The zero address (used for burns)
    pub const ZERO: Self = Self([0u8; 20]);

    /// Create an address from raw bytes
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Derive address from a public key
    #[must_use]
    pub fn from_public_key(pubkey: &PublicKey) -> Self {
        let hash = hash_data(pubkey.as_bytes());
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash.as_bytes()[..20]);
        Self(addr)
    }

    /// Get the underlying bytes
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Convert to hex string with 0x prefix
    #[must_use]
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.0))
    }

    /// Parse from hex string (with or without 0x prefix)
    ///
    /// # Errors
    /// Returns error if hex is invalid or wrong length
    pub fn from_hex(s: &str) -> Result<Self, AddressError> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(s).map_err(|_| AddressError::InvalidHex)?;

        if bytes.len() != 20 {
            return Err(AddressError::InvalidLength(bytes.len()));
        }

        let mut arr = [0u8; 20];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Check if this is the zero/burn address
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 20]
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.to_hex())
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Address parsing errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum AddressError {
    /// Invalid hex encoding
    #[error("invalid hex encoding")]
    InvalidHex,
    /// Invalid address length
    #[error("invalid address length: expected 20 bytes, got {0}")]
    InvalidLength(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_address_from_pubkey() {
        let kp = Keypair::generate();
        let addr = Address::from_public_key(kp.public_key());

        // Same pubkey should give same address
        let addr2 = Address::from_public_key(kp.public_key());
        assert_eq!(addr, addr2);
    }

    #[test]
    fn test_address_hex_roundtrip() {
        let kp = Keypair::generate();
        let addr = Address::from_public_key(kp.public_key());

        let hex = addr.to_hex();
        let parsed = Address::from_hex(&hex).unwrap();
        assert_eq!(addr, parsed);
    }

    #[test]
    fn test_zero_address() {
        assert!(Address::ZERO.is_zero());

        let kp = Keypair::generate();
        let addr = Address::from_public_key(kp.public_key());
        assert!(!addr.is_zero());
    }
}
