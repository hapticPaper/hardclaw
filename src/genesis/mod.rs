//! Genesis bootstrapping for the `HardClaw` blockchain.
//!
//! The genesis block IS a bounty task: the job of bootstrapping the network
//! over a 30-day period. This module implements the tiered airdrop,
//! liveness-gated vesting, competency challenges, the DNS break-glass
//! mechanism, and the bootstrap state machine.

pub mod airdrop;
pub mod bootstrap;
pub mod bounty;
pub mod competency;
pub mod config;
pub mod contracts;
pub mod liveness;
pub mod vesting;

use serde::{Deserialize, Serialize};

use crate::crypto::{hash_data, Hash, PublicKey};
use crate::types::{Address, HclawAmount, Timestamp};

/// Duration of the bootstrap period (30 days in milliseconds)
pub const BOOTSTRAP_DURATION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// One day in milliseconds
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Number of days in the bootstrap period
pub const BOOTSTRAP_DAYS: u32 = 30;

/// SIMPLIFIED GENESIS: Flat 100 HCLAW stake for first 5,000 wallets
pub const GENESIS_AIRDROP_AMOUNT: u64 = 100;

/// Maximum genesis participants
pub const MAX_GENESIS_PARTICIPANTS: u32 = 5_000;

/// Total airdrop pool: 500,000 HCLAW (5,000 × 100)
pub const AIRDROP_POOL_HCLAW: u64 = MAX_GENESIS_PARTICIPANTS as u64 * GENESIS_AIRDROP_AMOUNT;

/// Minimum stake required (50 HCLAW)
pub const MINIMUM_STAKE_HCLAW: u64 = 50;

/// Maximum DNS break-glass bootstrap nodes
pub const MAX_DNS_BOOTSTRAP_NODES: u32 = 10;

/// Tokens per DNS break-glass node
pub const DNS_BOOTSTRAP_TOKENS: u64 = 250_000;

/// DNS break-glass vesting period (24 hours)
pub const DNS_BOOTSTRAP_VESTING_MS: i64 = DAY_MS;

/// DNS domain that authorizes bootstrap nodes
pub const BOOTSTRAP_DNS_DOMAIN: &str = "clawpaper.com";

/// The genesis bootstrap job — the chain's first task
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootstrapJob {
    /// Deterministic job ID
    pub id: Hash,
    /// Human-readable description
    pub description: String,
    /// Total bounty (the airdrop pool)
    pub total_bounty: HclawAmount,
    /// Completion criteria
    pub completion: BootstrapCompletionCriteria,
}

impl BootstrapJob {
    /// Create the bootstrap job with a deterministic ID
    #[must_use]
    pub fn new(total_bounty: HclawAmount, bootstrap_duration_ms: i64) -> Self {
        let id = hash_data(b"hardclaw-genesis-bootstrap-v1");
        Self {
            id,
            description: "Bootstrap the HardClaw network: onboard verifiers, \
                          distribute airdrop via daily bounty curve over 90 days."
                .to_string(),
            total_bounty,
            completion: BootstrapCompletionCriteria {
                max_duration_ms: bootstrap_duration_ms,
                target_verifiers: MAX_GENESIS_PARTICIPANTS,
                min_verifiers: 5, // Minimum for network health
            },
        }
    }
}

/// When the bootstrap job is considered complete
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootstrapCompletionCriteria {
    /// Maximum duration (30 days from genesis)
    pub max_duration_ms: i64,
    /// Target verifier count (early completion if reached)
    pub target_verifiers: u32,
    /// Minimum verifiers needed for a healthy network
    pub min_verifiers: u32,
}

/// Configuration for the DNS break-glass mechanism.
///
/// Up to 10 additional bootstrap nodes can be authorized by adding them
/// to DNS records on the clawpaper.com domain. Each receives 250K tokens
/// with a 24-hour vesting period. This is an emergency mechanism for
/// bringing authoritative nodes online or injecting liquidity.
///
/// Security: DNS resolution alone is NOT sufficient. The DNS TXT record
/// must contain a signature over the node's public key, signed by the
/// genesis authority key. This prevents DNS hijacking from claiming tokens.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnsBreakGlassConfig {
    /// Domain to resolve bootstrap nodes from
    pub domain: String,
    /// Maximum additional nodes that can be authorized
    pub max_nodes: u32,
    /// Tokens per DNS bootstrap node
    pub tokens_each: HclawAmount,
    /// Vesting period (24 hours)
    pub vesting_ms: i64,
    /// Authority public key — DNS TXT records must contain a signature
    /// from this key over the node's public key to be valid.
    /// This protects against DNS hijacking.
    pub authority_key: PublicKey,
}

/// A DNS break-glass claim
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DnsBootstrapClaim {
    /// Node address
    pub address: Address,
    /// Node public key
    pub node_key: PublicKey,
    /// DNS hostname that resolved to this node
    pub hostname: String,
    /// Tokens allocated
    pub amount: HclawAmount,
    /// When claimed
    pub claimed_at: Timestamp,
    /// When fully vested (`claimed_at` + 24h)
    pub vests_at: Timestamp,
}

/// The full genesis configuration embedded in block 0
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Chain identifier (prevents replay across networks)
    pub chain_id: String,
    /// The bootstrap job
    pub bootstrap_job: BootstrapJob,
    /// Flat airdrop amount (100 HCLAW per participant)
    pub airdrop_amount: HclawAmount,
    /// Maximum participants (5,000)
    pub max_participants: u32,
    /// Pre-approved verifier addresses (skip competency check)
    pub pre_approved: Vec<Address>,
    /// DNS break-glass configuration
    pub dns_break_glass: DnsBreakGlassConfig,
    /// Bootstrap period start (genesis block timestamp)
    pub bootstrap_start: Timestamp,
    /// Bootstrap period end
    pub bootstrap_end: Timestamp,
    /// Protocol version at genesis
    pub protocol_version: u32,
    /// Whether to deploy genesis contracts in the genesis block
    #[serde(default)]
    pub deploy_contracts: bool,
    /// Initial total voting power (sum of all stakes)
    /// Only used if `deploy_contracts` is true
    #[serde(default)]
    pub initial_voting_power: u128,
}

impl GenesisConfig {
    /// Create the default genesis config
    #[must_use]
    pub fn new(
        chain_id: String,
        pre_approved: Vec<Address>,
        authority_key: PublicKey,
        now: Timestamp,
    ) -> Self {
        let airdrop_amount = HclawAmount::from_hclaw(GENESIS_AIRDROP_AMOUNT);
        let total_bounty = HclawAmount::from_hclaw(AIRDROP_POOL_HCLAW);

        Self {
            chain_id,
            bootstrap_job: BootstrapJob::new(total_bounty, BOOTSTRAP_DURATION_MS),
            airdrop_amount,
            max_participants: MAX_GENESIS_PARTICIPANTS,
            pre_approved,
            dns_break_glass: DnsBreakGlassConfig {
                domain: BOOTSTRAP_DNS_DOMAIN.to_string(),
                max_nodes: MAX_DNS_BOOTSTRAP_NODES,
                tokens_each: HclawAmount::from_hclaw(DNS_BOOTSTRAP_TOKENS),
                vesting_ms: DNS_BOOTSTRAP_VESTING_MS,
                authority_key,
            },
            bootstrap_start: now,
            bootstrap_end: now + BOOTSTRAP_DURATION_MS,
            protocol_version: 1,
            deploy_contracts: false, // default to opt-in
            initial_voting_power: 0,
        }
    }

    /// Compute a deterministic hash of this config (for genesis block hash)
    #[must_use]
    pub fn config_hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(self.chain_id.as_bytes());
        data.extend_from_slice(self.bootstrap_job.id.as_bytes());
        data.extend_from_slice(&self.bootstrap_start.to_le_bytes());
        data.extend_from_slice(&self.bootstrap_end.to_le_bytes());
        data.extend_from_slice(&self.airdrop_amount.raw().to_le_bytes());
        data.extend_from_slice(&self.max_participants.to_le_bytes());
        for addr in &self.pre_approved {
            data.extend_from_slice(addr.as_bytes());
        }
        data.extend_from_slice(self.dns_break_glass.authority_key.as_bytes());
        hash_data(&data)
    }

    /// Validate the config
    pub fn validate(&self) -> Result<(), GenesisError> {
        if self.chain_id.is_empty() {
            return Err(GenesisError::InvalidConfig("chain_id is empty".into()));
        }
        if self.max_participants == 0 {
            return Err(GenesisError::InvalidConfig(
                "max_participants is zero".into(),
            ));
        }
        if self.airdrop_amount.raw() == 0 {
            return Err(GenesisError::InvalidConfig("airdrop_amount is zero".into()));
        }
        if self.dns_break_glass.max_nodes > 10 {
            return Err(GenesisError::InvalidConfig(
                "DNS break-glass max_nodes cannot exceed 10".into(),
            ));
        }
        Ok(())
    }

    /// Total maximum supply that could be minted at genesis
    /// (airdrop pool + full DNS break-glass reserve)
    #[must_use]
    pub fn max_genesis_supply(&self) -> HclawAmount {
        let airdrop_total =
            HclawAmount::from_raw(self.airdrop_amount.raw() * self.max_participants as u128);
        let dns_reserve = HclawAmount::from_raw(
            self.dns_break_glass.tokens_each.raw() * self.dns_break_glass.max_nodes as u128,
        );
        airdrop_total.saturating_add(dns_reserve)
    }
}

/// Genesis-related errors
#[derive(Debug, thiserror::Error)]
pub enum GenesisError {
    /// Invalid genesis configuration
    #[error("invalid genesis config: {0}")]
    InvalidConfig(String),
    /// Bootstrap period not active
    #[error("bootstrap period not active")]
    BootstrapNotActive,
    /// Airdrop exhausted
    #[error("all airdrop positions have been claimed")]
    AirdropExhausted,
    /// Address already claimed
    #[error("address has already claimed an airdrop")]
    AlreadyClaimed,
    /// Competency challenge failed
    #[error("competency challenge: {0}")]
    CompetencyFailed(String),
    /// DNS break-glass limit reached
    #[error("DNS break-glass: all {MAX_DNS_BOOTSTRAP_NODES} slots have been used")]
    DnsBreakGlassExhausted,
    /// Invalid DNS break-glass claim
    #[error("DNS break-glass: {0}")]
    DnsBreakGlassInvalid(String),
    /// Liveness requirement not met
    #[error("liveness requirement not met for day {day}")]
    LivenessNotMet {
        /// The day that failed liveness
        day: u32,
    },
    /// IO error (config file loading)
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parse error
    #[error("config parse error: {0}")]
    ParseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_addresses(n: usize) -> Vec<Address> {
        (0..n)
            .map(|_| Address::from_public_key(Keypair::generate().public_key()))
            .collect()
    }

    #[test]
    fn test_flat_airdrop_pool() {
        assert_eq!(
            AIRDROP_POOL_HCLAW,
            MAX_GENESIS_PARTICIPANTS as u64 * GENESIS_AIRDROP_AMOUNT
        );
        assert_eq!(AIRDROP_POOL_HCLAW, 500_000);
    }

    #[test]
    fn test_genesis_config_hash_deterministic() {
        let addrs = test_addresses(2);
        let authority = Keypair::generate();
        let cfg1 = GenesisConfig::new(
            "test-1".into(),
            addrs.clone(),
            authority.public_key().clone(),
            1000,
        );
        let cfg2 = GenesisConfig::new("test-1".into(), addrs, authority.public_key().clone(), 1000);
        assert_eq!(cfg1.config_hash(), cfg2.config_hash());
    }

    #[test]
    fn test_genesis_config_validation() {
        let addrs = test_addresses(5);
        let authority = Keypair::generate();
        let cfg = GenesisConfig::new("test".into(), addrs, authority.public_key().clone(), 1000);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_max_genesis_supply() {
        let addrs = test_addresses(5);
        let authority = Keypair::generate();
        let cfg = GenesisConfig::new("test".into(), addrs, authority.public_key().clone(), 1000);
        // 500,000 airdrop + 10 * 250,000 DNS = 3,000,000
        assert_eq!(cfg.max_genesis_supply().whole_hclaw(), 3_000_000);
    }

    #[test]
    fn test_bootstrap_job_deterministic_id() {
        let job1 = BootstrapJob::new(HclawAmount::from_hclaw(100), BOOTSTRAP_DURATION_MS);
        let job2 = BootstrapJob::new(HclawAmount::from_hclaw(200), BOOTSTRAP_DURATION_MS);
        assert_eq!(job1.id, job2.id);
    }

    #[test]
    fn test_flat_allocation() {
        assert_eq!(GENESIS_AIRDROP_AMOUNT, 100);
        assert_eq!(MAX_GENESIS_PARTICIPANTS, 5_000);
        assert_eq!(MINIMUM_STAKE_HCLAW, 50);
    }
}
