//! Digital signatures using ML-DSA-65 (FIPS 204).
//!
//! Post-quantum signatures from genesis. ML-DSA-65 provides NIST Level 3
//! security against both classical and quantum adversaries.
//!
//! Uses the RustCrypto `ml-dsa` crate (pure Rust, deterministic keygen from seed).
//! Ed25519 is retained ONLY for libp2p transport identity (in network module).

use ml_dsa::{
    EncodedSignature, EncodedVerifyingKey, MlDsa65,
    Signature as MlDsaSignature, SigningKey as MlDsaSigningKey,
    VerifyingKey as MlDsaVerifyingKey, B32,
};
use rand::RngCore;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash, Hasher as StdHasher};
use zeroize::Zeroize;

use super::{CryptoError, CryptoResult};

/// ML-DSA-65 public key size in bytes (FIPS 204)
pub const PUBKEY_SIZE: usize = 1952;
/// ML-DSA-65 signature size in bytes (FIPS 204)
pub const SIGNATURE_SIZE: usize = 3309;
/// ML-DSA-65 seed size in bytes (for deterministic keygen)
pub const SEED_SIZE: usize = 32;
/// ML-DSA-65 secret key size in bytes (32-byte seed format)
pub const SECRET_KEY_SIZE: usize = SEED_SIZE;

/// An ML-DSA-65 digital signature
#[derive(Clone, PartialEq, Eq)]
pub struct Signature(Vec<u8>);

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&hex::encode(&self.0))
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
            if bytes.len() != SIGNATURE_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "signature must be {} bytes, got {}",
                    SIGNATURE_SIZE,
                    bytes.len()
                )));
            }
            Ok(Self(bytes))
        } else {
            let bytes = <Vec<u8>>::deserialize(deserializer)?;
            if bytes.len() != SIGNATURE_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "signature must be {} bytes, got {}",
                    SIGNATURE_SIZE,
                    bytes.len()
                )));
            }
            Ok(Self(bytes))
        }
    }
}

impl Signature {
    /// Create from raw bytes
    ///
    /// # Errors
    /// Returns error if bytes are not the correct length
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        if bytes.len() != SIGNATURE_SIZE {
            return Err(CryptoError::InvalidSignature);
        }
        Ok(Self(bytes.to_vec()))
    }

    /// Create a placeholder (all zeros) for unsigned structures.
    /// This is NOT a valid signature — it's a sentinel for "not yet signed".
    #[must_use]
    pub fn placeholder() -> Self {
        Self(vec![0u8; SIGNATURE_SIZE])
    }

    /// Get underlying bytes
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert to hex string
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
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

/// An ML-DSA-65 public key (1952 bytes)
#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey(Vec<u8>);

impl Hash for PublicKey {
    fn hash<H: StdHasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&hex::encode(&self.0))
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
            if bytes.len() != PUBKEY_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "public key must be {} bytes, got {}",
                    PUBKEY_SIZE,
                    bytes.len()
                )));
            }
            Ok(Self(bytes))
        } else {
            let bytes = <Vec<u8>>::deserialize(deserializer)?;
            if bytes.len() != PUBKEY_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "public key must be {} bytes, got {}",
                    PUBKEY_SIZE,
                    bytes.len()
                )));
            }
            Ok(Self(bytes))
        }
    }
}

impl PublicKey {
    /// Create from raw bytes (validated)
    ///
    /// # Errors
    /// Returns error if bytes don't represent a valid ML-DSA-65 public key
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        if bytes.len() != PUBKEY_SIZE {
            return Err(CryptoError::InvalidPublicKey(format!(
                "expected {} bytes, got {}",
                PUBKEY_SIZE,
                bytes.len()
            )));
        }
        Ok(Self(bytes.to_vec()))
    }

    /// Create from raw bytes without validation (for deserialization)
    #[must_use]
    pub fn from_bytes_unchecked(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    /// Get underlying bytes
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert to hex string
    #[must_use]
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Parse from hex string
    ///
    /// # Errors
    /// Returns error if hex is invalid or not a valid public key
    pub fn from_hex(s: &str) -> CryptoResult<Self> {
        let bytes = hex::decode(s).map_err(|e| CryptoError::InvalidPublicKey(e.to_string()))?;
        Self::from_bytes(&bytes)
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

/// An ML-DSA-65 secret key (32-byte seed format)
///
/// SECURITY: This type intentionally does not implement Clone or Debug
/// to prevent accidental key leakage. Memory is zeroized on drop.
pub struct SecretKey(Vec<u8>);

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl SecretKey {
    /// Create from raw bytes
    ///
    /// # Errors
    /// Returns error if bytes are invalid
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        if bytes.len() != SECRET_KEY_SIZE {
            return Err(CryptoError::InvalidPublicKey(format!(
                "invalid secret key: expected {} bytes, got {}",
                SECRET_KEY_SIZE,
                bytes.len()
            )));
        }
        Ok(Self(bytes.to_vec()))
    }

    /// Get underlying bytes
    ///
    /// # Security
    /// Be careful with the returned bytes - they are the raw secret key material.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    /// Sign a message
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        let sk = restore_signing_key(&self.0);
        let sig = sk
            .sign_deterministic(message, &[])
            .expect("signing should not fail with valid key");
        let encoded = sig.encode();
        Signature(AsRef::<[u8]>::as_ref(&encoded).to_vec())
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
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        let result = Self::from_seed(&seed);
        seed.zeroize();
        result
    }

    /// Deterministically generate a keypair from a 32-byte seed.
    ///
    /// Same seed always produces the same keypair (FIPS 204 ML-DSA.KeyGen(d)).
    /// This enables BIP39 mnemonic -> seed -> deterministic keypair recovery.
    #[must_use]
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let b32 = B32::from(*seed);
        let sk = MlDsaSigningKey::<MlDsa65>::from_seed(&b32);
        let vk_bytes = sk.verifying_key().encode();
        let public = PublicKey(AsRef::<[u8]>::as_ref(&vk_bytes).to_vec());
        let secret = SecretKey(seed.to_vec());
        Self { secret, public }
    }

    /// Create from an existing secret key and public key pair
    #[must_use]
    pub fn from_parts(secret: SecretKey, public: PublicKey) -> Self {
        Self { secret, public }
    }

    /// Get the public key
    #[must_use]
    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// Sign a message
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.secret.sign(message)
    }

    /// Get the secret key (for persistence)
    #[must_use]
    pub fn secret_key(&self) -> &SecretKey {
        &self.secret
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
    let vk_encoded = EncodedVerifyingKey::<MlDsa65>::try_from(public_key.0.as_slice())
        .map_err(|_| CryptoError::InvalidPublicKey("wrong length".into()))?;
    let vk = MlDsaVerifyingKey::<MlDsa65>::decode(&vk_encoded);

    let sig_encoded = EncodedSignature::<MlDsa65>::try_from(signature.0.as_slice())
        .map_err(|_| CryptoError::InvalidSignature)?;
    let sig = MlDsaSignature::<MlDsa65>::decode(&sig_encoded)
        .ok_or(CryptoError::InvalidSignature)?;

    use ml_dsa::signature::Verifier;
    vk.verify(message, &sig)
        .map_err(|_| CryptoError::InvalidSignature)
}

/// Restore a SigningKey from 32-byte seed format
fn restore_signing_key(bytes: &[u8]) -> MlDsaSigningKey<MlDsa65> {
    let seed: [u8; 32] = bytes.try_into()
        .expect("SecretKey should always contain 32 bytes");
    let b32 = B32::from(seed);
    MlDsaSigningKey::<MlDsa65>::from_seed(&b32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_sizes() {
        let keypair = Keypair::generate();
        assert_eq!(keypair.public_key().as_bytes().len(), PUBKEY_SIZE);
        assert_eq!(keypair.secret_key().to_bytes().len(), SECRET_KEY_SIZE);

        let sig = keypair.sign(b"test");
        assert_eq!(sig.as_bytes().len(), SIGNATURE_SIZE);
    }

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
    fn test_pubkey_hex_roundtrip() {
        let keypair = Keypair::generate();
        let hex_str = keypair.public_key().to_hex();
        let parsed = PublicKey::from_hex(&hex_str).unwrap();
        assert_eq!(keypair.public_key(), &parsed);
    }

    #[test]
    fn test_pubkey_bytes_roundtrip() {
        let keypair = Keypair::generate();
        let bytes = keypair.public_key().as_bytes();
        let parsed = PublicKey::from_bytes(bytes).unwrap();
        assert_eq!(keypair.public_key(), &parsed);
    }

    #[test]
    fn test_secret_key_roundtrip() {
        let keypair = Keypair::generate();
        let sk_bytes = keypair.secret_key().to_bytes();
        let pk = keypair.public_key().clone();
        let restored = SecretKey::from_bytes(&sk_bytes).unwrap();
        let restored_kp = Keypair::from_parts(restored, pk);
        assert_eq!(keypair.public_key(), restored_kp.public_key());
        // Verify signing still works
        let sig = restored_kp.sign(b"roundtrip test");
        assert!(verify(restored_kp.public_key(), b"roundtrip test", &sig).is_ok());
    }

    #[test]
    fn test_placeholder_signature() {
        let sig = Signature::placeholder();
        assert_eq!(sig.as_bytes().len(), SIGNATURE_SIZE);
    }

    #[test]
    fn test_deterministic_keygen_from_seed() {
        let seed = [42u8; 32];
        let kp1 = Keypair::from_seed(&seed);
        let kp2 = Keypair::from_seed(&seed);

        // Same seed must produce identical keypairs
        assert_eq!(kp1.public_key(), kp2.public_key());
        assert_eq!(kp1.secret_key().to_bytes(), kp2.secret_key().to_bytes());

        // Different seed must produce different keypairs
        let kp3 = Keypair::from_seed(&[99u8; 32]);
        assert_ne!(kp1.public_key(), kp3.public_key());
    }

    #[test]
    fn test_seed_derived_sign_verify() {
        let seed = [7u8; 32];
        let kp = Keypair::from_seed(&seed);
        let msg = b"deterministic signing test";
        let sig = kp.sign(msg);
        assert!(verify(kp.public_key(), msg, &sig).is_ok());
    }
}
