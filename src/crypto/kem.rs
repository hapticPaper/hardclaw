//! Key Encapsulation Mechanism using HQC-192.
//!
//! Post-quantum key exchange for peer-to-peer encryption.
//! HQC-192 provides NIST Level 3 security (NIST 4th PQ selection).
//!
//! Used for chain-level encrypted communication between peers,
//! complementing the Ed25519-based libp2p transport layer.

use pqcrypto_hqc::hqc192;
use pqcrypto_traits::kem::{
    Ciphertext as PqCiphertext, PublicKey as PqKemPublicKey, SecretKey as PqKemSecretKey,
    SharedSecret as PqSharedSecret,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroize;

use super::{CryptoError, CryptoResult};

/// HQC-192 public key size in bytes
pub const KEM_PUBKEY_SIZE: usize = 4522;
/// HQC-192 secret key size in bytes
pub const KEM_SECRET_KEY_SIZE: usize = 4586;
/// HQC-192 ciphertext size in bytes
pub const KEM_CIPHERTEXT_SIZE: usize = 8978;
/// HQC-192 shared secret size in bytes
pub const KEM_SHARED_SECRET_SIZE: usize = 64;

/// An HQC-192 KEM public key (4522 bytes)
#[derive(Clone, PartialEq, Eq)]
pub struct KemPublicKey(Vec<u8>);

impl Serialize for KemPublicKey {
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

impl<'de> Deserialize<'de> for KemPublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            let s = String::deserialize(deserializer)?;
            let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
            if bytes.len() != KEM_PUBKEY_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "KEM public key must be {} bytes, got {}",
                    KEM_PUBKEY_SIZE,
                    bytes.len()
                )));
            }
            Ok(Self(bytes))
        } else {
            let bytes = <Vec<u8>>::deserialize(deserializer)?;
            if bytes.len() != KEM_PUBKEY_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "KEM public key must be {} bytes, got {}",
                    KEM_PUBKEY_SIZE,
                    bytes.len()
                )));
            }
            Ok(Self(bytes))
        }
    }
}

impl KemPublicKey {
    /// Create from raw bytes (validated)
    ///
    /// # Errors
    /// Returns error if bytes are not the correct length
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        hqc192::PublicKey::from_bytes(bytes)
            .map_err(|e| CryptoError::InvalidPublicKey(format!("invalid KEM public key: {e}")))?;
        Ok(Self(bytes.to_vec()))
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

    fn pq_key(&self) -> CryptoResult<hqc192::PublicKey> {
        hqc192::PublicKey::from_bytes(&self.0)
            .map_err(|e| CryptoError::InvalidPublicKey(format!("invalid KEM public key: {e}")))
    }
}

impl std::fmt::Debug for KemPublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KemPubKey({}..)", &self.to_hex()[..16])
    }
}

/// An HQC-192 KEM secret key (4586 bytes)
///
/// SECURITY: Memory is zeroized on drop.
pub struct KemSecretKey(Vec<u8>);

impl Drop for KemSecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl KemSecretKey {
    /// Create from raw bytes
    ///
    /// # Errors
    /// Returns error if bytes are invalid
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        hqc192::SecretKey::from_bytes(bytes)
            .map_err(|e| CryptoError::InvalidPublicKey(format!("invalid KEM secret key: {e}")))?;
        Ok(Self(bytes.to_vec()))
    }

    /// Get underlying bytes
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    fn pq_key(&self) -> CryptoResult<hqc192::SecretKey> {
        hqc192::SecretKey::from_bytes(&self.0)
            .map_err(|e| CryptoError::InvalidPublicKey(format!("invalid KEM secret key: {e}")))
    }
}

/// An HQC-192 ciphertext (8978 bytes)
#[derive(Clone, PartialEq, Eq)]
pub struct KemCiphertext(Vec<u8>);

impl KemCiphertext {
    /// Create from raw bytes
    ///
    /// # Errors
    /// Returns error if bytes are invalid
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        hqc192::Ciphertext::from_bytes(bytes)
            .map_err(|_| CryptoError::InvalidSignature)?;
        Ok(Self(bytes.to_vec()))
    }

    /// Get underlying bytes
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A shared secret derived from HQC-192 KEM (64 bytes)
pub struct SharedSecret(Vec<u8>);

impl Drop for SharedSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl SharedSecret {
    /// Get underlying bytes
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// An HQC-192 KEM keypair
pub struct KemKeypair {
    secret: KemSecretKey,
    public: KemPublicKey,
}

impl KemKeypair {
    /// Generate a new random KEM keypair
    #[must_use]
    pub fn generate() -> Self {
        let (pk, sk) = hqc192::keypair();
        let public = KemPublicKey(pk.as_bytes().to_vec());
        let secret = KemSecretKey(sk.as_bytes().to_vec());
        Self { secret, public }
    }

    /// Get the public key
    #[must_use]
    pub fn public_key(&self) -> &KemPublicKey {
        &self.public
    }

    /// Get the secret key (for persistence)
    #[must_use]
    pub fn secret_key(&self) -> &KemSecretKey {
        &self.secret
    }

    /// Decapsulate a ciphertext to recover the shared secret
    ///
    /// # Errors
    /// Returns error if the ciphertext or secret key is invalid
    pub fn decapsulate(&self, ciphertext: &KemCiphertext) -> CryptoResult<SharedSecret> {
        decapsulate(ciphertext, &self.secret)
    }
}

/// Encapsulate: generate a shared secret and ciphertext for a recipient's public key
///
/// # Errors
/// Returns error if the public key is invalid
pub fn encapsulate(public_key: &KemPublicKey) -> CryptoResult<(SharedSecret, KemCiphertext)> {
    let pq_pk = public_key.pq_key()?;
    let (ss, ct) = hqc192::encapsulate(&pq_pk);
    Ok((
        SharedSecret(ss.as_bytes().to_vec()),
        KemCiphertext(ct.as_bytes().to_vec()),
    ))
}

/// Decapsulate: recover the shared secret from a ciphertext using the secret key
///
/// # Errors
/// Returns error if the ciphertext or secret key is invalid
pub fn decapsulate(ciphertext: &KemCiphertext, secret_key: &KemSecretKey) -> CryptoResult<SharedSecret> {
    let pq_sk = secret_key.pq_key()?;
    let pq_ct = hqc192::Ciphertext::from_bytes(&ciphertext.0)
        .map_err(|_| CryptoError::InvalidSignature)?;
    let ss = hqc192::decapsulate(&pq_ct, &pq_sk);
    Ok(SharedSecret(ss.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kem_sizes() {
        let (pk, sk) = hqc192::keypair();
        assert_eq!(pk.as_bytes().len(), KEM_PUBKEY_SIZE);
        assert_eq!(sk.as_bytes().len(), KEM_SECRET_KEY_SIZE);

        let (ss, ct) = hqc192::encapsulate(&pk);
        assert_eq!(ct.as_bytes().len(), KEM_CIPHERTEXT_SIZE);
        assert_eq!(ss.as_bytes().len(), KEM_SHARED_SECRET_SIZE);
    }

    #[test]
    fn test_encapsulate_decapsulate() {
        let keypair = KemKeypair::generate();

        let (shared_secret_sender, ciphertext) = encapsulate(keypair.public_key()).unwrap();
        let shared_secret_receiver = keypair.decapsulate(&ciphertext).unwrap();

        assert_eq!(
            shared_secret_sender.as_bytes(),
            shared_secret_receiver.as_bytes()
        );
    }

    #[test]
    fn test_different_keypairs_produce_different_secrets() {
        let keypair1 = KemKeypair::generate();
        let keypair2 = KemKeypair::generate();

        let (ss1, _ct1) = encapsulate(keypair1.public_key()).unwrap();
        let (ss2, _ct2) = encapsulate(keypair2.public_key()).unwrap();

        // Different keypairs produce different shared secrets
        assert_ne!(ss1.as_bytes(), ss2.as_bytes());
    }

    #[test]
    fn test_pubkey_bytes_roundtrip() {
        let keypair = KemKeypair::generate();
        let bytes = keypair.public_key().as_bytes();
        let parsed = KemPublicKey::from_bytes(bytes).unwrap();
        assert_eq!(keypair.public_key(), &parsed);
    }
}
