//! Commitment schemes for Schelling Point consensus.
//!
//! Uses SHA3-256 for commitment hashing (different from BLAKE3 used elsewhere)
//! to provide domain separation and use a standardized hash for commitments.

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use super::{hash::Hash, CryptoError, CryptoResult};

/// A cryptographic commitment to a value
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Commitment(Hash);

impl Commitment {
    /// Create a commitment from value and randomness
    ///
    /// Uses: SHA3-256(value || nonce)
    #[must_use]
    pub fn create<T: AsRef<[u8]>>(value: T, nonce: &[u8; 32]) -> Self {
        let mut hasher = Sha3_256::new();
        hasher.update(value.as_ref());
        hasher.update(nonce);
        let result = hasher.finalize();

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self(Hash::from_bytes(bytes))
    }

    /// Verify that a value and nonce match this commitment
    ///
    /// # Errors
    /// Returns error if the commitment doesn't match
    pub fn verify<T: AsRef<[u8]>>(&self, value: T, nonce: &[u8; 32]) -> CryptoResult<()> {
        let expected = Self::create(value, nonce);
        if self.0 == expected.0 {
            Ok(())
        } else {
            Err(CryptoError::CommitmentMismatch)
        }
    }

    /// Get the underlying hash
    #[must_use]
    pub const fn as_hash(&self) -> &Hash {
        &self.0
    }
}

/// A commit-reveal scheme for blind voting
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitReveal<T> {
    /// The commitment (hash of value + nonce)
    pub commitment: Commitment,
    /// The revealed value (None until revealed)
    pub value: Option<T>,
    /// The nonce used (None until revealed)
    pub nonce: Option<[u8; 32]>,
}

impl<T: AsRef<[u8]> + Clone> CommitReveal<T> {
    /// Create a new commit-reveal with a random nonce
    #[must_use]
    pub fn commit(value: T) -> Self {
        let nonce: [u8; 32] = rand::random();
        let commitment = Commitment::create(&value, &nonce);

        Self {
            commitment,
            value: Some(value),
            nonce: Some(nonce),
        }
    }

    /// Create a commitment-only view (for sharing before reveal)
    #[must_use]
    pub fn commitment_only(&self) -> Self {
        Self {
            commitment: self.commitment,
            value: None,
            nonce: None,
        }
    }

    /// Reveal the value and nonce
    ///
    /// # Errors
    /// Returns error if already revealed or values don't match commitment
    pub fn reveal(&mut self, value: T, nonce: [u8; 32]) -> CryptoResult<&T> {
        self.commitment.verify(&value, &nonce)?;
        self.value = Some(value);
        self.nonce = Some(nonce);
        Ok(self.value.as_ref().expect("just set"))
    }

    /// Check if this commitment has been revealed
    #[must_use]
    pub const fn is_revealed(&self) -> bool {
        self.value.is_some()
    }

    /// Get the revealed value if available
    #[must_use]
    pub const fn revealed_value(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

/// Generate cryptographically secure random nonce
#[must_use]
pub fn generate_nonce() -> [u8; 32] {
    rand::random()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commitment_verify() {
        let value = b"my vote";
        let nonce: [u8; 32] = rand::random();

        let commitment = Commitment::create(value, &nonce);
        assert!(commitment.verify(value, &nonce).is_ok());
    }

    #[test]
    fn test_commitment_wrong_value() {
        let nonce: [u8; 32] = rand::random();
        let commitment = Commitment::create(b"original", &nonce);

        assert!(commitment.verify(b"tampered", &nonce).is_err());
    }

    #[test]
    fn test_commitment_wrong_nonce() {
        let value = b"test";
        let nonce1: [u8; 32] = rand::random();
        let nonce2: [u8; 32] = rand::random();

        let commitment = Commitment::create(value, &nonce1);
        assert!(commitment.verify(value, &nonce2).is_err());
    }

    #[test]
    fn test_commit_reveal_flow() {
        let value = b"secret vote".to_vec();
        let cr = CommitReveal::commit(value.clone());

        // Before reveal, only commitment is visible
        let public = cr.commitment_only();
        assert!(!public.is_revealed());

        // Verify the reveal
        let mut public = public;
        assert!(public.reveal(
            value.clone(),
            cr.nonce.expect("should have nonce")
        ).is_ok());

        assert!(public.is_revealed());
        assert_eq!(public.revealed_value(), Some(&value));
    }
}
