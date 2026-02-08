//! Cryptographic primitives for `HardClaw` protocol.
//!
//! Post-quantum from genesis:
//! - ML-DSA-65 (Dilithium3) for digital signatures (NIST FIPS 204)
//! - HQC-192 for key encapsulation (NIST 4th PQ selection)
//! - BLAKE3 for fast hashing
//! - SHA3-256 for commitment schemes
//! - bip39 for standard mnemonic seed phrases
//! - Ed25519 retained ONLY for libp2p transport identity

mod commitment;
mod hash;
pub mod kem;
mod mnemonic;
mod signature;

pub use commitment::{CommitReveal, Commitment};
pub use hash::{hash_data, merkle_root, Hash, Hasher};
pub use kem::{
    decapsulate, encapsulate, KemCiphertext, KemKeypair, KemPublicKey, KemSecretKey, SharedSecret,
    KEM_CIPHERTEXT_SIZE, KEM_PUBKEY_SIZE, KEM_SECRET_KEY_SIZE, KEM_SHARED_SECRET_SIZE,
};
pub use mnemonic::{
    generate_mnemonic, keypair_from_mnemonic, keypair_from_phrase, mnemonic_to_words,
    parse_mnemonic, MNEMONIC_WORD_COUNT,
};
pub use signature::{
    sign, verify, Keypair, PublicKey, SecretKey, Signature, PUBKEY_SIZE, SECRET_KEY_SIZE,
    SEED_SIZE, SIGNATURE_SIZE,
};

use thiserror::Error;

/// Cryptographic errors
#[derive(Debug, Error)]
pub enum CryptoError {
    /// Invalid signature
    #[error("invalid signature")]
    InvalidSignature,
    /// Invalid public key format
    #[error("invalid public key: {0}")]
    InvalidPublicKey(String),
    /// Invalid hash format
    #[error("invalid hash: {0}")]
    InvalidHash(String),
    /// Commitment verification failed
    #[error("commitment verification failed")]
    CommitmentMismatch,
    /// Serialization error
    #[error("serialization error: {0}")]
    Serialization(String),
    /// Invalid mnemonic phrase
    #[error("invalid mnemonic: {0}")]
    InvalidMnemonic(String),
}

/// Result type for crypto operations
pub type CryptoResult<T> = Result<T, CryptoError>;
