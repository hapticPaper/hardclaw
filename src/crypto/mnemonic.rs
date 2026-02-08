//! BIP39 mnemonic seed phrase support for wallet recovery.
//!
//! Provides standard 24-word seed phrases that deterministically derive
//! ML-DSA-65 keypairs via: mnemonic -> BIP39 seed -> BLAKE3 KDF -> ML-DSA seed.
//!
//! The same mnemonic always produces the same wallet.

use bip39::{Language, Mnemonic};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::RngCore;
use sha2::{Digest, Sha256};

use super::{CryptoError, CryptoResult, Keypair};

/// Number of words in the mnemonic (24 words = 256 bits of entropy)
pub const MNEMONIC_WORD_COUNT: usize = 24;

/// Domain separator for ML-DSA key derivation from BIP39 seed
const ML_DSA_KDF_DOMAIN: &[u8] = b"hardclaw-ml-dsa-keygen-v1";

/// Generate a new random mnemonic phrase.
///
/// Returns a 24-word BIP39 mnemonic using the English word list.
#[must_use]
pub fn generate_mnemonic() -> Mnemonic {
    let entropy_bytes = MNEMONIC_WORD_COUNT * 4 / 3; // 24 words => 32 bytes
    let mut entropy = vec![0u8; entropy_bytes];
    rand::rngs::OsRng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy_in(Language::English, &entropy)
        .expect("entropy length is valid for 24-word mnemonic")
}

/// Parse a mnemonic phrase from a string.
///
/// # Errors
/// Returns error if the phrase is invalid (wrong words, checksum, etc.)
pub fn parse_mnemonic(phrase: &str) -> CryptoResult<Mnemonic> {
    Mnemonic::parse_in(Language::English, phrase)
        .map_err(|e| CryptoError::InvalidMnemonic(e.to_string()))
}

/// Derive an Ed25519 keypair from a mnemonic (for libp2p transport identity).
///
/// This returns raw Ed25519 bytes, NOT an ML-DSA keypair. Used only by
/// the network module for libp2p peer identity derivation.
#[must_use]
#[allow(dead_code)] // Used in tests and potentially by network module
pub fn derive_ed25519_from_mnemonic(mnemonic: &Mnemonic, passphrase: &str) -> ([u8; 32], [u8; 32]) {
    let seed = mnemonic.to_seed(passphrase);

    let mut hasher = Sha256::new();
    hasher.update(seed);
    let hash = hasher.finalize();

    let mut ed25519_seed = [0u8; 32];
    ed25519_seed.copy_from_slice(&hash[..32]);

    let signing_key = SigningKey::from_bytes(&ed25519_seed);
    let verifying_key: VerifyingKey = (&signing_key).into();

    (ed25519_seed, verifying_key.to_bytes())
}

/// Deterministically derive an ML-DSA-65 keypair from a mnemonic.
///
/// Derivation path: BIP39 mnemonic -> 64-byte seed (with passphrase)
/// -> BLAKE3(domain || `bip39_seed`) -> 32-byte ML-DSA seed
/// -> ML-DSA.KeyGen(seed) -> deterministic (pk, sk)
///
/// The same mnemonic + passphrase always produces the same wallet.
#[must_use]
pub fn keypair_from_mnemonic(mnemonic: &Mnemonic, passphrase: &str) -> Keypair {
    let bip39_seed = mnemonic.to_seed(passphrase);

    // Derive 32-byte ML-DSA seed via BLAKE3 with domain separation
    let mut hasher = blake3::Hasher::new();
    hasher.update(ML_DSA_KDF_DOMAIN);
    hasher.update(&bip39_seed);
    let ml_dsa_seed: [u8; 32] = *hasher.finalize().as_bytes();

    Keypair::from_seed(&ml_dsa_seed)
}

/// Derive a keypair from a mnemonic phrase string.
///
/// # Errors
/// Returns error if the phrase is invalid
pub fn keypair_from_phrase(phrase: &str, passphrase: &str) -> CryptoResult<Keypair> {
    let mnemonic = parse_mnemonic(phrase)?;
    Ok(keypair_from_mnemonic(&mnemonic, passphrase))
}

/// Convert a mnemonic to its word list.
#[must_use]
pub fn mnemonic_to_words(mnemonic: &Mnemonic) -> Vec<&'static str> {
    mnemonic.words().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mnemonic() {
        let mnemonic = generate_mnemonic();
        assert_eq!(mnemonic.words().count(), 24);
    }

    #[test]
    fn test_mnemonic_roundtrip() {
        let mnemonic = generate_mnemonic();
        let phrase = mnemonic.to_string();
        let parsed = parse_mnemonic(&phrase).unwrap();
        assert_eq!(mnemonic.to_string(), parsed.to_string());
    }

    #[test]
    fn test_ed25519_derivation_deterministic() {
        let mnemonic = generate_mnemonic();
        let (seed1, pk1) = derive_ed25519_from_mnemonic(&mnemonic, "");
        let (seed2, pk2) = derive_ed25519_from_mnemonic(&mnemonic, "");
        assert_eq!(seed1, seed2);
        assert_eq!(pk1, pk2);
    }

    #[test]
    fn test_ed25519_passphrase_changes_key() {
        let mnemonic = generate_mnemonic();
        let (_, pk1) = derive_ed25519_from_mnemonic(&mnemonic, "");
        let (_, pk2) = derive_ed25519_from_mnemonic(&mnemonic, "secret");
        assert_ne!(pk1, pk2);
    }

    #[test]
    fn test_mnemonic_to_ml_dsa_deterministic() {
        let mnemonic = generate_mnemonic();
        let kp1 = keypair_from_mnemonic(&mnemonic, "");
        let kp2 = keypair_from_mnemonic(&mnemonic, "");

        // Same mnemonic must produce identical keypairs
        assert_eq!(kp1.public_key(), kp2.public_key());
        assert_eq!(kp1.secret_key().to_bytes(), kp2.secret_key().to_bytes());
    }

    #[test]
    fn test_mnemonic_passphrase_changes_ml_dsa_key() {
        let mnemonic = generate_mnemonic();
        let kp1 = keypair_from_mnemonic(&mnemonic, "");
        let kp2 = keypair_from_mnemonic(&mnemonic, "secret");
        assert_ne!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn test_mnemonic_derived_sign_verify() {
        let mnemonic = generate_mnemonic();
        let kp = keypair_from_mnemonic(&mnemonic, "");
        let msg = b"mnemonic recovery test";
        let sig = kp.sign(msg);
        assert!(super::super::verify(kp.public_key(), msg, &sig).is_ok());
    }

    #[test]
    fn test_phrase_roundtrip_produces_same_wallet() {
        let mnemonic = generate_mnemonic();
        let phrase = mnemonic.to_string();
        let kp1 = keypair_from_mnemonic(&mnemonic, "");
        let kp2 = keypair_from_phrase(&phrase, "").unwrap();
        assert_eq!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn test_invalid_mnemonic() {
        let result = parse_mnemonic("invalid mnemonic phrase");
        assert!(result.is_err());
    }
}
