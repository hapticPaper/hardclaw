//! Hashing primitives using BLAKE3 for performance.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte hash digest
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    /// The zero hash (used as genesis parent)
    pub const ZERO: Self = Self([0u8; 32]);

    /// Create a hash from raw bytes
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    ///
    /// # Errors
    /// Returns error if hex string is invalid or wrong length
    pub fn from_hex(s: &str) -> Result<Self, super::CryptoError> {
        let bytes = hex::decode(s)
            .map_err(|e| super::CryptoError::InvalidHash(e.to_string()))?;

        if bytes.len() != 32 {
            return Err(super::CryptoError::InvalidHash(
                format!("expected 32 bytes, got {}", bytes.len())
            ));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", &self.to_hex()[..16])
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Hasher for incremental hashing
pub struct Hasher {
    inner: blake3::Hasher,
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher {
    /// Create a new hasher
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: blake3::Hasher::new(),
        }
    }

    /// Update the hasher with data
    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        self.inner.update(data);
        self
    }

    /// Finalize and get the hash
    #[must_use]
    pub fn finalize(self) -> Hash {
        let result = self.inner.finalize();
        Hash::from_bytes(*result.as_bytes())
    }
}

/// Hash arbitrary data
#[must_use]
pub fn hash_data(data: &[u8]) -> Hash {
    let result = blake3::hash(data);
    Hash::from_bytes(*result.as_bytes())
}

/// Compute Merkle root from a list of hashes
///
/// Uses a standard binary Merkle tree construction.
/// Empty list returns zero hash.
#[must_use]
pub fn merkle_root(hashes: &[Hash]) -> Hash {
    if hashes.is_empty() {
        return Hash::ZERO;
    }

    if hashes.len() == 1 {
        return hashes[0];
    }

    let mut current_level: Vec<Hash> = hashes.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

        for chunk in current_level.chunks(2) {
            let combined = if chunk.len() == 2 {
                let mut hasher = Hasher::new();
                hasher.update(chunk[0].as_bytes());
                hasher.update(chunk[1].as_bytes());
                hasher.finalize()
            } else {
                // Odd number: hash with itself
                let mut hasher = Hasher::new();
                hasher.update(chunk[0].as_bytes());
                hasher.update(chunk[0].as_bytes());
                hasher.finalize()
            };
            next_level.push(combined);
        }

        current_level = next_level;
    }

    current_level[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let data = b"test data";
        let h1 = hash_data(data);
        let h2 = hash_data(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_different_data() {
        let h1 = hash_data(b"data1");
        let h2 = hash_data(b"data2");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_merkle_empty() {
        assert_eq!(merkle_root(&[]), Hash::ZERO);
    }

    #[test]
    fn test_merkle_single() {
        let h = hash_data(b"single");
        assert_eq!(merkle_root(&[h]), h);
    }

    #[test]
    fn test_merkle_deterministic() {
        let hashes: Vec<Hash> = (0..10)
            .map(|i| hash_data(&[i as u8]))
            .collect();

        let root1 = merkle_root(&hashes);
        let root2 = merkle_root(&hashes);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_hex_roundtrip() {
        let original = hash_data(b"test");
        let hex_str = original.to_hex();
        let parsed = Hash::from_hex(&hex_str).unwrap();
        assert_eq!(original, parsed);
    }
}
