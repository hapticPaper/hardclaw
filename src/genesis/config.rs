//! TOML configuration loading/saving for genesis.
//!
//! Genesis configs can be loaded from TOML files, allowing different
//! configurations for testnet vs mainnet. The TOML format mirrors the
//! `GenesisConfig` struct.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    GenesisError, BOOTSTRAP_DNS_DOMAIN, BOOTSTRAP_DURATION_MS, DNS_BOOTSTRAP_TOKENS,
    GENESIS_AIRDROP_AMOUNT, MAX_DNS_BOOTSTRAP_NODES, MAX_GENESIS_PARTICIPANTS,
};

/// TOML-serializable genesis config (simplified - no tiers)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenesisConfigToml {
    /// Chain identifier
    pub chain_id: String,
    /// Bootstrap duration in days (default: 30)
    #[serde(default = "default_bootstrap_days")]
    pub bootstrap_duration_days: u32,
    /// Flat airdrop amount (default: 100 HCLAW)
    #[serde(default = "default_airdrop_amount")]
    pub airdrop_amount: u64,
    /// Maximum participants (default: 5,000)
    #[serde(default = "default_max_participants")]
    pub max_participants: u32,
    /// Pre-approved verifier addresses (hex-encoded)
    pub pre_approved: Vec<String>,
    /// Authority public key for DNS break-glass (hex-encoded)
    pub authority_key: String,
    /// DNS break-glass config (optional, uses defaults if absent)
    pub dns_break_glass: Option<DnsBreakGlassToml>,
}

/// TOML-serializable DNS break-glass config
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnsBreakGlassToml {
    /// Domain (default: clawpaper.com)
    #[serde(default = "default_dns_domain")]
    pub domain: String,
    /// Max nodes (default: 10)
    #[serde(default = "default_dns_max_nodes")]
    pub max_nodes: u32,
    /// Tokens per node (whole HCLAW, default: 250,000)
    #[serde(default = "default_dns_tokens")]
    pub tokens_each: u64,
    /// Vesting period in hours (default: 24)
    #[serde(default = "default_dns_vesting_hours")]
    pub vesting_hours: u32,
}

fn default_bootstrap_days() -> u32 {
    30
}

fn default_airdrop_amount() -> u64 {
    GENESIS_AIRDROP_AMOUNT
}

fn default_max_participants() -> u32 {
    MAX_GENESIS_PARTICIPANTS
}

fn default_dns_domain() -> String {
    BOOTSTRAP_DNS_DOMAIN.to_string()
}

fn default_dns_max_nodes() -> u32 {
    MAX_DNS_BOOTSTRAP_NODES
}

fn default_dns_tokens() -> u64 {
    DNS_BOOTSTRAP_TOKENS
}

fn default_dns_vesting_hours() -> u32 {
    24
}

impl GenesisConfigToml {
    /// Load from a TOML file
    pub fn load_from_file(path: &Path) -> Result<Self, GenesisError> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| GenesisError::ParseError(e.to_string()))
    }

    /// Save to a TOML file
    pub fn save_to_file(&self, path: &Path) -> Result<(), GenesisError> {
        let content =
            toml::to_string_pretty(self).map_err(|e| GenesisError::ParseError(e.to_string()))?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Create a default testnet TOML config (for quick local testing)
#[must_use]
pub fn default_testnet_toml() -> GenesisConfigToml {
    GenesisConfigToml {
        chain_id: "hardclaw-testnet-1".to_string(),
        bootstrap_duration_days: 30,
        airdrop_amount: GENESIS_AIRDROP_AMOUNT,
        max_participants: MAX_GENESIS_PARTICIPANTS,
        pre_approved: vec!["<bootstrap-addr-1>".into(); 5],
        authority_key: "<authority-pubkey-hex>".to_string(),
        dns_break_glass: None,
    }
}

/// Compute the bootstrap duration in ms from days
#[must_use]
pub fn bootstrap_duration_from_days(days: u32) -> i64 {
    if days == 30 {
        BOOTSTRAP_DURATION_MS
    } else {
        days as i64 * 24 * 60 * 60 * 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_roundtrip() {
        let config = default_testnet_toml();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: GenesisConfigToml = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.chain_id, config.chain_id);
        assert_eq!(deserialized.airdrop_amount, GENESIS_AIRDROP_AMOUNT);
        assert_eq!(deserialized.max_participants, MAX_GENESIS_PARTICIPANTS);
    }

    #[test]
    fn test_flat_allocation() {
        let config = default_testnet_toml();
        assert_eq!(config.airdrop_amount, 100);
        assert_eq!(config.max_participants, 5_000);
    }
}
