use crate::crypto::PublicKey;
use crate::types::{Address, HclawAmount, Timestamp};
use serde::{Deserialize, Serialize};

/// Configuration for the DNS break-glass mechanism.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
    pub authority_key: PublicKey,
}

/// A DNS break-glass claim
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DnsBreakGlassClaim {
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
