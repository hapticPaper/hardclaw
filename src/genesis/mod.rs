//! Genesis bootstrapping for the `HardClaw` blockchain.
//!
//! The genesis block IS a bounty task: the job of bootstrapping the network
//! over a 30-day period. This module contains the bounty payout curve,
//! TOML-based genesis configuration, and contract initialization.

pub mod bounty;
pub mod config;
pub mod contracts;

use serde::{Deserialize, Serialize};

use crate::crypto::{hash_data, Hash, PublicKey};
use crate::types::{Address, HclawAmount, Timestamp};

/// Duration of the bootstrap period (30 days in milliseconds)
pub const BOOTSTRAP_DURATION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// One day in milliseconds
pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Number of days in the bootstrap period
pub const BOOTSTRAP_DAYS: u32 = 30;

/// Founder airdrop: 250,000 HCLAW for the first 8 pre-approved wallets
pub const FOUNDER_AIRDROP_AMOUNT: u64 = 250_000;

/// Maximum founder wallets (pre-approved, get the large airdrop)
pub const MAX_FOUNDER_WALLETS: u32 = 8;

/// Standard airdrop: 100 HCLAW for everyone after the founders
pub const GENESIS_AIRDROP_AMOUNT: u64 = 100;

/// Maximum genesis participants (includes founders)
pub const MAX_GENESIS_PARTICIPANTS: u32 = 5_000;

/// Total airdrop pool: 8 × 250K + 4,992 × 100 = 2,499,200 HCLAW
pub const AIRDROP_POOL_HCLAW: u64 = MAX_FOUNDER_WALLETS as u64 * FOUNDER_AIRDROP_AMOUNT
    + (MAX_GENESIS_PARTICIPANTS - MAX_FOUNDER_WALLETS) as u64 * GENESIS_AIRDROP_AMOUNT;

/// Minimum stake required (50 HCLAW)
pub const MINIMUM_STAKE_HCLAW: u64 = 50;

/// Bootstrap node allocation: 500,000 HCLAW each
pub const BOOTSTRAP_NODE_TOKENS: u64 = 500_000;

/// Number of initial bootstrap nodes
pub const NUM_BOOTSTRAP_NODES: u32 = 4;

/// Maximum DNS break-glass bootstrap nodes (emergency mechanism, separate from initial nodes)
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

/// Genesis-related errors
#[derive(Debug, thiserror::Error)]
pub enum GenesisError {
    /// Invalid genesis configuration
    #[error("invalid genesis config: {0}")]
    InvalidConfig(String),
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

    #[test]
    fn test_tiered_airdrop_pool() {
        // 8 founders × 250K + 4,992 regular × 100 = 2,499,200
        let founder_pool = MAX_FOUNDER_WALLETS as u64 * FOUNDER_AIRDROP_AMOUNT;
        let regular_pool =
            (MAX_GENESIS_PARTICIPANTS - MAX_FOUNDER_WALLETS) as u64 * GENESIS_AIRDROP_AMOUNT;
        assert_eq!(AIRDROP_POOL_HCLAW, founder_pool + regular_pool);
        assert_eq!(AIRDROP_POOL_HCLAW, 2_499_200);
    }

    #[test]
    fn test_dns_break_glass_config() {
        let authority = Keypair::generate();
        let cfg = DnsBreakGlassConfig {
            domain: BOOTSTRAP_DNS_DOMAIN.to_string(),
            max_nodes: MAX_DNS_BOOTSTRAP_NODES,
            tokens_each: HclawAmount::from_hclaw(DNS_BOOTSTRAP_TOKENS),
            vesting_ms: DNS_BOOTSTRAP_VESTING_MS,
            authority_key: authority.public_key().clone(),
        };
        assert_eq!(cfg.max_nodes, 10);
        assert_eq!(cfg.tokens_each.whole_hclaw(), 250_000);
    }

    #[test]
    fn test_max_genesis_supply() {
        // 4 bootstrap × 500K + 8 founders × 250K + 4,992 regular × 100 + 10 DNS × 250K
        let bootstrap = NUM_BOOTSTRAP_NODES as u64 * BOOTSTRAP_NODE_TOKENS;
        let airdrop = AIRDROP_POOL_HCLAW;
        let dns = MAX_DNS_BOOTSTRAP_NODES as u64 * DNS_BOOTSTRAP_TOKENS;
        // 2,000,000 + 2,499,200 + 2,500,000 = 6,999,200
        assert_eq!(bootstrap + airdrop + dns, 6_999_200);
    }

    #[test]
    fn test_bootstrap_job_deterministic_id() {
        let job1 = BootstrapJob::new(HclawAmount::from_hclaw(100), BOOTSTRAP_DURATION_MS);
        let job2 = BootstrapJob::new(HclawAmount::from_hclaw(200), BOOTSTRAP_DURATION_MS);
        assert_eq!(job1.id, job2.id);
    }

    #[test]
    fn test_allocation_constants() {
        assert_eq!(FOUNDER_AIRDROP_AMOUNT, 250_000);
        assert_eq!(MAX_FOUNDER_WALLETS, 8);
        assert_eq!(GENESIS_AIRDROP_AMOUNT, 100);
        assert_eq!(MAX_GENESIS_PARTICIPANTS, 5_000);
        assert_eq!(BOOTSTRAP_NODE_TOKENS, 500_000);
        assert_eq!(NUM_BOOTSTRAP_NODES, 4);
        assert_eq!(MINIMUM_STAKE_HCLAW, 50);
    }
}
