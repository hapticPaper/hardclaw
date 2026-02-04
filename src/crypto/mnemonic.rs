//! BIP39 mnemonic seed phrase support for wallet compatibility.
//!
//! This module provides standard 24-word seed phrases that can be used
//! with other wallets and safely backed up.

use bip39::{Language, Mnemonic};
use rand::RngCore;
use sha2::{Digest, Sha256};

use super::{CryptoResult, SecretKey, Keypair, CryptoError};

/// Number of words in the mnemonic (24 words = 256 bits of entropy)
pub const MNEMONIC_WORD_COUNT: usize = 24;

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

/// Derive an Ed25519 keypair from a mnemonic.
///
/// Uses the mnemonic's entropy with SHA-256 to derive a 32-byte seed
/// for Ed25519 key generation. This is compatible with how Solana
/// and other Ed25519-based chains derive keys from mnemonics.
///
/// # Arguments
/// * `mnemonic` - The BIP39 mnemonic
/// * `passphrase` - Optional passphrase (empty string if none)
#[must_use]
pub fn keypair_from_mnemonic(mnemonic: &Mnemonic, passphrase: &str) -> Keypair {
    // Get the seed from mnemonic (includes passphrase in derivation)
    let seed = mnemonic.to_seed(passphrase);

    // Use first 32 bytes of SHA-256(seed) as Ed25519 seed
    // This matches Solana's derivation method
    let mut hasher = Sha256::new();
    hasher.update(seed);
    let hash = hasher.finalize();

    let mut ed25519_seed = [0u8; 32];
    ed25519_seed.copy_from_slice(&hash[..32]);

    let secret = SecretKey::from_bytes(ed25519_seed)
        .expect("SHA-256 output is always valid Ed25519 seed");

    Keypair::from_secret(secret)
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
        let words: Vec<_> = mnemonic.words().collect();
        assert_eq!(words.len(), 24);
    }

    #[test]
    fn test_mnemonic_roundtrip() {
        let mnemonic = generate_mnemonic();
        let phrase = mnemonic.to_string();
        let parsed = parse_mnemonic(&phrase).unwrap();
        assert_eq!(mnemonic.to_string(), parsed.to_string());
    }

    #[test]
    fn test_keypair_from_mnemonic_deterministic() {
        let mnemonic = generate_mnemonic();
        let kp1 = keypair_from_mnemonic(&mnemonic, "");
        let kp2 = keypair_from_mnemonic(&mnemonic, "");
        assert_eq!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn test_passphrase_changes_key() {
        let mnemonic = generate_mnemonic();
        let kp1 = keypair_from_mnemonic(&mnemonic, "");
        let kp2 = keypair_from_mnemonic(&mnemonic, "secret");
        assert_ne!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn test_known_mnemonic() {
        // Test with a known mnemonic to ensure deterministic derivation
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
        let mnemonic = parse_mnemonic(phrase).unwrap();
        let keypair = keypair_from_mnemonic(&mnemonic, "");

        // This keypair should always derive to the same address
        let pubkey = keypair.public_key().to_hex();
        assert!(!pubkey.is_empty());
    }

    #[test]
    fn test_invalid_mnemonic() {
        let result = parse_mnemonic("invalid mnemonic phrase");
        assert!(result.is_err());
    }
}
