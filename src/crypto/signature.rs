//! Digital signatures using Ed25519.
//!
//! Ed25519 is chosen for:
//! - Fast verification (important for high-frequency agent transactions)
//! - Small signatures (64 bytes)
//! - Deterministic signatures (same input = same signature)
//! - Battle-tested (used by Solana, Stellar, etc.)

use ed25519_dalek::{
    Signer as DalekSigner, SigningKey, Verifier as DalekVerifier, VerifyingKey,
};
use rand::rngs::OsRng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use super::{CryptoError, CryptoResult};

/// A 64-byte Ed25519 signature
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom("signature must be 64 bytes"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl Signature {
    /// Create from raw bytes
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Get underlying bytes
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    /// Convert to hex string
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sig({}..)", &self.to_hex()[..16])
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// A 32-byte Ed25519 public key
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    /// Create from raw bytes
    ///
    /// # Errors
    /// Returns error if bytes don't represent a valid curve point
    pub fn from_bytes(bytes: [u8; 32]) -> CryptoResult<Self> {
        // Validate it's a valid point on the curve
        VerifyingKey::from_bytes(&bytes)
            .map_err(|e| CryptoError::InvalidPublicKey(e.to_string()))?;
        Ok(Self(bytes))
    }

    /// Get underlying bytes (unchecked, for deserialization)
    #[must_use]
    pub const fn from_bytes_unchecked(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get underlying bytes
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
    /// Returns error if hex is invalid or not a valid public key
    pub fn from_hex(s: &str) -> CryptoResult<Self> {
        let bytes = hex::decode(s)
            .map_err(|e| CryptoError::InvalidPublicKey(e.to_string()))?;

        if bytes.len() != 32 {
            return Err(CryptoError::InvalidPublicKey(
                format!("expected 32 bytes, got {}", bytes.len())
            ));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Self::from_bytes(arr)
    }

    fn to_verifying_key(&self) -> CryptoResult<VerifyingKey> {
        VerifyingKey::from_bytes(&self.0)
            .map_err(|e| CryptoError::InvalidPublicKey(e.to_string()))
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PubKey({}..)", &self.to_hex()[..16])
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// A 32-byte Ed25519 secret key
///
/// SECURITY: This type intentionally does not implement Clone or Debug
/// to prevent accidental key leakage.
pub struct SecretKey(SigningKey);

impl SecretKey {
    /// Generate a new random secret key
    #[must_use]
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        Self(SigningKey::generate(&mut csprng))
    }

    /// Create from raw bytes
    ///
    /// # Errors
    /// Returns error if bytes are invalid
    pub fn from_bytes(bytes: [u8; 32]) -> CryptoResult<Self> {
        Ok(Self(SigningKey::from_bytes(&bytes)))
    }

    /// Get underlying bytes
    ///
    /// # Security
    /// Be careful with the returned bytes - they are the raw secret key material.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    /// Derive the public key
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        let verifying = self.0.verifying_key();
        PublicKey::from_bytes_unchecked(verifying.to_bytes())
    }

    /// Sign a message
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        let sig = self.0.sign(message);
        Signature::from_bytes(sig.to_bytes())
    }
}

/// A keypair containing both secret and public keys
pub struct Keypair {
    secret: SecretKey,
    public: PublicKey,
}

impl Keypair {
    /// Generate a new random keypair
    #[must_use]
    pub fn generate() -> Self {
        let secret = SecretKey::generate();
        let public = secret.public_key();
        Self { secret, public }
    }

    /// Create from an existing secret key
    #[must_use]
    pub fn from_secret(secret: SecretKey) -> Self {
        let public = secret.public_key();
        Self { secret, public }
    }

    /// Get the public key
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// Sign a message
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.secret.sign(message)
    }
}

/// Sign a message with a secret key (convenience function)
#[must_use]
pub fn sign(secret: &SecretKey, message: &[u8]) -> Signature {
    secret.sign(message)
}

/// Verify a signature against a public key and message
///
/// # Errors
/// Returns error if signature is invalid
pub fn verify(public_key: &PublicKey, message: &[u8], signature: &Signature) -> CryptoResult<()> {
    let verifying_key = public_key.to_verifying_key()?;
    let sig = ed25519_dalek::Signature::from_bytes(signature.as_bytes());

    verifying_key
        .verify(message, &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_verify() {
        let keypair = Keypair::generate();
        let message = b"test message";

        let sig = keypair.sign(message);
        assert!(verify(keypair.public_key(), message, &sig).is_ok());
    }

    #[test]
    fn test_wrong_message_fails() {
        let keypair = Keypair::generate();
        let sig = keypair.sign(b"original");

        assert!(verify(keypair.public_key(), b"tampered", &sig).is_err());
    }

    #[test]
    fn test_wrong_key_fails() {
        let keypair1 = Keypair::generate();
        let keypair2 = Keypair::generate();
        let message = b"test";

        let sig = keypair1.sign(message);
        assert!(verify(keypair2.public_key(), message, &sig).is_err());
    }

    #[test]
    fn test_deterministic_signatures() {
        let secret = SecretKey::generate();
        let message = b"deterministic";

        let sig1 = secret.sign(message);
        let sig2 = secret.sign(message);
        assert_eq!(sig1.as_bytes(), sig2.as_bytes());
    }

    #[test]
    fn test_pubkey_hex_roundtrip() {
        let keypair = Keypair::generate();
        let hex_str = keypair.public_key().to_hex();
        let parsed = PublicKey::from_hex(&hex_str).unwrap();
        assert_eq!(keypair.public_key(), &parsed);
    }
}
