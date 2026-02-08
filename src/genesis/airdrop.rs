//! Flat airdrop allocation and tracking.
//!
//! Simplified genesis: 100 HCLAW to first 5,000 verifiers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{GenesisError, GENESIS_AIRDROP_AMOUNT, MAX_GENESIS_PARTICIPANTS, MINIMUM_STAKE_HCLAW};
use crate::types::{Address, HclawAmount, Timestamp};

/// Simplified airdrop configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AirdropConfig {
    /// Flat amount each participant receives (100 HCLAW)
    pub amount_per_participant: HclawAmount,
    /// Maximum number of participants (5,000)
    pub max_participants: u32,
    /// Minimum stake required (50 HCLAW)
    pub min_stake: HclawAmount,
}

impl AirdropConfig {
    /// Create default flat airdrop config
    #[must_use]
    pub fn new() -> Self {
        Self {
            amount_per_participant: HclawAmount::from_hclaw(GENESIS_AIRDROP_AMOUNT),
            max_participants: MAX_GENESIS_PARTICIPANTS,
            min_stake: HclawAmount::from_hclaw(MINIMUM_STAKE_HCLAW),
        }
    }

    /// Total airdrop pool
    #[must_use]
    pub fn total_pool(&self) -> HclawAmount {
        HclawAmount::from_raw(self.amount_per_participant.raw() * self.max_participants as u128)
    }

    /// Minimum stake for all participants
    #[must_use]
    pub fn current_min_stake(&self) -> HclawAmount {
        self.min_stake
    }
}

impl Default for AirdropConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks airdrop claims during the bootstrap period
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AirdropTracker {
    /// Airdrop configuration
    config: AirdropConfig,
    /// Claims by address
    claims: HashMap<Address, AirdropClaim>,
    /// Next position to be assigned (1-indexed)
    next_position: u32,
    /// Total tokens distributed so far
    total_distributed: HclawAmount,
    /// Pre-approved addresses (skip competency check)
    pre_approved: Vec<Address>,
}

/// A single airdrop claim
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AirdropClaim {
    /// Address that claimed
    pub address: Address,
    /// Position in the airdrop (1-indexed)
    pub position: u32,
    /// Airdrop amount (100 HCLAW)
    pub amount: HclawAmount,
    /// When claimed
    pub claimed_at: Timestamp,
    /// Whether activated (post-competency check)
    pub activated: bool,
}

impl AirdropTracker {
    /// Create a new tracker
    #[must_use]
    pub fn new(pre_approved: Vec<Address>) -> Self {
        Self {
            config: AirdropConfig::new(),
            claims: HashMap::new(),
            next_position: 1,
            total_distributed: HclawAmount::ZERO,
            pre_approved,
        }
    }

    /// Reserve the next airdrop position for an address.
    /// Does not distribute tokens yet — that happens after competency check.
    pub fn reserve_position(
        &mut self,
        address: Address,
        now: Timestamp,
    ) -> Result<AirdropClaim, GenesisError> {
        // Check if already claimed
        if self.claims.contains_key(&address) {
            return Err(GenesisError::AlreadyClaimed);
        }

        // Check if exhausted
        if self.next_position > self.config.max_participants {
            return Err(GenesisError::AirdropExhausted);
        }

        let claim = AirdropClaim {
            address,
            position: self.next_position,
            amount: self.config.amount_per_participant,
            claimed_at: now,
            activated: false,
        };

        self.claims.insert(address, claim.clone());
        self.next_position += 1;

        Ok(claim)
    }

    /// Activate a claim (after competency check passes).
    /// Returns the amount to credit to the verifier.
    pub fn activate_claim(&mut self, address: &Address) -> Result<HclawAmount, GenesisError> {
        let claim = self.claims.get_mut(address).ok_or_else(|| {
            GenesisError::InvalidConfig(format!("no claim found for address {address}"))
        })?;

        if claim.activated {
            return Err(GenesisError::AlreadyClaimed);
        }

        claim.activated = true;
        self.total_distributed = self.total_distributed.saturating_add(claim.amount);

        Ok(claim.amount)
    }

    /// Get a claim by address
    #[must_use]
    pub fn get_claim(&self, address: &Address) -> Option<&AirdropClaim> {
        self.claims.get(address)
    }

    /// Next position to be assigned
    #[must_use]
    pub fn next_position(&self) -> u32 {
        self.next_position
    }

    /// How many positions have been reserved
    #[must_use]
    pub fn claimed_count(&self) -> u32 {
        self.claims.len() as u32
    }

    /// How many claims have been activated
    #[must_use]
    pub fn activated_count(&self) -> u32 {
        self.claims.values().filter(|c| c.activated).count() as u32
    }

    /// Total tokens distributed so far
    #[must_use]
    pub fn total_distributed(&self) -> HclawAmount {
        self.total_distributed
    }

    /// Whether all positions have been claimed
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.next_position > self.config.max_participants
    }

    /// Current min stake (flat for all participants)
    #[must_use]
    pub fn current_min_stake(&self) -> HclawAmount {
        self.config.current_min_stake()
    }

    /// Check if an address is pre-approved
    #[must_use]
    pub fn is_pre_approved(&self, address: &Address) -> bool {
        self.pre_approved.contains(address)
    }

    /// Get the config
    #[must_use]
    pub fn config(&self) -> &AirdropConfig {
        &self.config
    }

    /// Calculate unclaimed tokens (for burning at bootstrap end)
    #[must_use]
    pub fn unclaimed_pool(&self) -> HclawAmount {
        let total_pool = self.config.total_pool();
        total_pool.saturating_sub(self.total_distributed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_address(seed: u8) -> Address {
        Address::from_public_key(Keypair::from_seed(&[seed; 32]).public_key())
    }

    #[test]
    fn test_flat_config() {
        let config = AirdropConfig::new();
        assert_eq!(config.amount_per_participant.whole_hclaw(), 100);
        assert_eq!(config.max_participants, 5_000);
        assert_eq!(config.min_stake.whole_hclaw(), 50);
        assert_eq!(config.total_pool().whole_hclaw(), 500_000);
    }

    #[test]
    fn test_reserve_and_activate() {
        let mut tracker = AirdropTracker::new(vec![]);
        let addr = test_address(1);

        let claim = tracker.reserve_position(addr, 1000).unwrap();
        assert_eq!(claim.position, 1);
        assert_eq!(claim.amount.whole_hclaw(), 100);
        assert!(!claim.activated);

        let amount = tracker.activate_claim(&addr).unwrap();
        assert_eq!(amount.whole_hclaw(), 100);
        assert_eq!(tracker.activated_count(), 1);
        assert_eq!(tracker.total_distributed().whole_hclaw(), 100);
    }

    #[test]
    fn test_already_claimed() {
        let mut tracker = AirdropTracker::new(vec![]);
        let addr = test_address(1);

        tracker.reserve_position(addr, 1000).unwrap();
        let result = tracker.reserve_position(addr, 1001);
        assert!(matches!(result, Err(GenesisError::AlreadyClaimed)));
    }

    #[test]
    fn test_exhaustion() {
        let mut tracker = AirdropTracker::new(vec![]);

        // Fill all slots - use u32 seed to ensure unique addresses
        for i in 0..MAX_GENESIS_PARTICIPANTS {
            let seed_bytes = i.to_le_bytes();
            let mut seed = [0u8; 32];
            seed[..4].copy_from_slice(&seed_bytes);
            let keypair = Keypair::from_seed(&seed);
            let addr = Address::from_public_key(keypair.public_key());
            tracker.reserve_position(addr, 1000).unwrap();
        }

        assert!(tracker.is_exhausted());

        // Next one should fail
        let result = tracker.reserve_position(test_address(255), 1000);
        assert!(matches!(result, Err(GenesisError::AirdropExhausted)));
    }

    #[test]
    fn test_pre_approved() {
        let addr = test_address(1);
        let tracker = AirdropTracker::new(vec![addr]);

        assert!(tracker.is_pre_approved(&addr));
        assert!(!tracker.is_pre_approved(&test_address(2)));
    }

    #[test]
    fn test_unclaimed_pool() {
        let mut tracker = AirdropTracker::new(vec![]);

        // Claim 100 positions
        for i in 0..100u8 {
            let addr = test_address(i);
            tracker.reserve_position(addr, 1000).unwrap();
            tracker.activate_claim(&addr).unwrap();
        }

        assert_eq!(tracker.total_distributed().whole_hclaw(), 10_000);
        assert_eq!(tracker.unclaimed_pool().whole_hclaw(), 490_000);
    }
}
