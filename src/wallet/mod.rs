//! Wallet management for HardClaw.
//!
//! Handles key generation, storage, and loading.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::crypto::{Keypair, PublicKey, SecretKey, Signature};
use crate::types::Address;

/// Wallet file format
#[derive(Serialize, Deserialize)]
struct WalletFile {
    /// Version for future compatibility
    version: u8,
    /// Public key (hex)
    public_key: String,
    /// Secret key (hex) - in production, this would be encrypted
    secret_key: String,
    /// Optional wallet name/label
    name: Option<String>,
    /// Creation timestamp
    created_at: i64,
}

/// A HardClaw wallet
pub struct Wallet {
    /// Secret key bytes (for persistence)
    secret_bytes: [u8; 32],
    /// The underlying keypair
    keypair: Keypair,
    /// Wallet name/label
    pub name: Option<String>,
    /// Path to wallet file (if loaded from disk)
    pub path: Option<PathBuf>,
}

impl Wallet {
    /// Generate a new wallet
    #[must_use]
    pub fn generate() -> Self {
        let secret = SecretKey::generate();
        let secret_bytes = secret.to_bytes();
        let keypair = Keypair::from_secret(secret);

        Self {
            secret_bytes,
            keypair,
            name: None,
            path: None,
        }
    }

    /// Generate with a name
    #[must_use]
    pub fn generate_with_name(name: String) -> Self {
        let mut wallet = Self::generate();
        wallet.name = Some(name);
        wallet
    }

    /// Create from secret key bytes
    ///
    /// # Errors
    /// Returns error if bytes are invalid
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Result<Self, WalletError> {
        let secret = SecretKey::from_bytes(bytes)
            .map_err(|e| WalletError::InvalidKey(e.to_string()))?;
        let keypair = Keypair::from_secret(secret);

        Ok(Self {
            secret_bytes: bytes,
            keypair,
            name: None,
            path: None,
        })
    }

    /// Get the public key
    #[must_use]
    pub fn public_key(&self) -> &PublicKey {
        self.keypair.public_key()
    }

    /// Get the address
    #[must_use]
    pub fn address(&self) -> Address {
        Address::from_public_key(self.keypair.public_key())
    }

    /// Get the underlying keypair
    #[must_use]
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    /// Sign a message
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.keypair.sign(message)
    }

    /// Save wallet to a file
    ///
    /// # Errors
    /// Returns error if file cannot be written
    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<(), WalletError> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| WalletError::IoError(e.to_string()))?;
        }

        let wallet_file = WalletFile {
            version: 1,
            public_key: self.keypair.public_key().to_hex(),
            secret_key: hex::encode(self.secret_bytes),
            name: self.name.clone(),
            created_at: crate::types::now_millis(),
        };

        let json = serde_json::to_string_pretty(&wallet_file)
            .map_err(|e| WalletError::SerializationError(e.to_string()))?;

        let mut file = File::create(path)
            .map_err(|e| WalletError::IoError(e.to_string()))?;

        file.write_all(json.as_bytes())
            .map_err(|e| WalletError::IoError(e.to_string()))?;

        self.path = Some(path.to_path_buf());
        Ok(())
    }

    /// Load wallet from a file
    ///
    /// # Errors
    /// Returns error if file cannot be read or is invalid
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, WalletError> {
        let path = path.as_ref();

        let mut file = File::open(path)
            .map_err(|e| WalletError::IoError(e.to_string()))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| WalletError::IoError(e.to_string()))?;

        let wallet_file: WalletFile = serde_json::from_str(&contents)
            .map_err(|e| WalletError::SerializationError(e.to_string()))?;

        if wallet_file.version != 1 {
            return Err(WalletError::UnsupportedVersion(wallet_file.version));
        }

        let secret_bytes_vec = hex::decode(&wallet_file.secret_key)
            .map_err(|e| WalletError::InvalidKey(e.to_string()))?;

        if secret_bytes_vec.len() != 32 {
            return Err(WalletError::InvalidKey(
                format!("expected 32 bytes, got {}", secret_bytes_vec.len())
            ));
        }

        let mut secret_bytes = [0u8; 32];
        secret_bytes.copy_from_slice(&secret_bytes_vec);

        let secret = SecretKey::from_bytes(secret_bytes)
            .map_err(|e| WalletError::InvalidKey(e.to_string()))?;

        let keypair = Keypair::from_secret(secret);

        // Verify public key matches
        if keypair.public_key().to_hex() != wallet_file.public_key {
            return Err(WalletError::InvalidKey(
                "public key mismatch".to_string()
            ));
        }

        Ok(Self {
            secret_bytes,
            keypair,
            name: wallet_file.name,
            path: Some(path.to_path_buf()),
        })
    }

    /// Get the default wallet directory
    #[must_use]
    pub fn default_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".hardclaw")
            .join("wallets")
    }

    /// Get the default wallet path
    #[must_use]
    pub fn default_path() -> PathBuf {
        Self::default_dir().join("default.json")
    }

    /// Check if default wallet exists
    #[must_use]
    pub fn default_exists() -> bool {
        Self::default_path().exists()
    }

    /// Load the default wallet
    ///
    /// # Errors
    /// Returns error if default wallet doesn't exist or is invalid
    pub fn load_default() -> Result<Self, WalletError> {
        Self::load(Self::default_path())
    }

    /// Save as the default wallet
    ///
    /// # Errors
    /// Returns error if wallet cannot be saved
    pub fn save_as_default(&mut self) -> Result<(), WalletError> {
        self.save(Self::default_path())
    }

    /// List all wallets in the default directory
    ///
    /// # Errors
    /// Returns error if directory cannot be read
    pub fn list_wallets() -> Result<Vec<WalletInfo>, WalletError> {
        let dir = Self::default_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut wallets = Vec::new();
        for entry in fs::read_dir(&dir)
            .map_err(|e| WalletError::IoError(e.to_string()))?
        {
            let entry = entry.map_err(|e| WalletError::IoError(e.to_string()))?;
            let path = entry.path();

            if path.extension().map_or(false, |e| e == "json") {
                if let Ok(wallet) = Self::load(&path) {
                    let name = wallet.name.clone().unwrap_or_else(|| {
                        path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string()
                    });
                    wallets.push(WalletInfo {
                        name,
                        address: wallet.address(),
                        public_key: wallet.public_key().to_hex(),
                        path,
                    });
                }
            }
        }

        Ok(wallets)
    }
}

/// Information about a wallet (without sensitive data)
#[derive(Clone, Debug)]
pub struct WalletInfo {
    /// Wallet name
    pub name: String,
    /// Wallet address
    pub address: Address,
    /// Public key (hex)
    pub public_key: String,
    /// Path to wallet file
    pub path: PathBuf,
}

/// Wallet errors
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    /// IO error
    #[error("IO error: {0}")]
    IoError(String),
    /// Serialization error
    #[error("serialization error: {0}")]
    SerializationError(String),
    /// Invalid key
    #[error("invalid key: {0}")]
    InvalidKey(String),
    /// Unsupported wallet version
    #[error("unsupported wallet version: {0}")]
    UnsupportedVersion(u8),
    /// Wallet not found
    #[error("wallet not found")]
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_wallet_generation() {
        let wallet = Wallet::generate();
        assert!(!wallet.public_key().to_hex().is_empty());
    }

    #[test]
    fn test_wallet_save_load() {
        let mut wallet = Wallet::generate_with_name("test".to_string());
        let original_pubkey = wallet.public_key().to_hex();

        let path = temp_dir().join("hardclaw_test_wallet.json");
        wallet.save(&path).unwrap();

        let loaded = Wallet::load(&path).unwrap();
        assert_eq!(loaded.public_key().to_hex(), original_pubkey);
        assert_eq!(loaded.name, Some("test".to_string()));

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_wallet_signing() {
        let wallet = Wallet::generate();
        let message = b"hello hardclaw";
        let signature = wallet.sign(message);

        assert!(crate::crypto::verify(wallet.public_key(), message, &signature).is_ok());
    }
}
