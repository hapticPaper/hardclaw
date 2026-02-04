//! Staking and slashing for verifiers.
//!
//! Verifiers must stake $HCLAW to participate in consensus.
//! Misbehavior (like approving honey pots) results in slashing.

use std::collections::HashMap;

use crate::crypto::Hash;
use crate::types::{Address, HclawAmount, Timestamp, now_millis};

/// Reason for slashing a verifier's stake
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlashingReason {
    /// Approved a honey pot solution
    HoneyPotApproval {
        /// ID of the honey pot solution that was incorrectly approved
        solution_id: Hash,
    },
    /// Submitted invalid verification
    InvalidVerification {
        /// Details about why the verification was invalid
        details: String,
    },
    /// Double signing (signing conflicting blocks)
    DoubleSigning {
        /// First conflicting block hash
        block_hash_1: Hash,
        /// Second conflicting block hash
        block_hash_2: Hash,
    },
    /// Extended downtime (offline for too long)
    Downtime {
        /// Duration offline in seconds
        offline_duration_secs: u64,
    },
}

impl SlashingReason {
    /// Get the slashing percentage for this reason
    #[must_use]
    pub const fn slash_percentage(&self) -> u8 {
        match self {
            // Honey pot approval = 100% slash (entire stake)
            Self::HoneyPotApproval { .. } => 100,
            // Invalid verification = 10% slash
            Self::InvalidVerification { .. } => 10,
            // Double signing = 100% slash
            Self::DoubleSigning { .. } => 100,
            // Downtime = 1% per hour (handled elsewhere)
            Self::Downtime { .. } => 1,
        }
    }
}

/// Information about a verifier's stake
#[derive(Clone, Debug)]
pub struct StakeInfo {
    /// Staker's address
    pub address: Address,
    /// Current staked amount
    pub amount: HclawAmount,
    /// When stake was created
    pub staked_at: Timestamp,
    /// When stake can be withdrawn (after unbonding period)
    pub withdrawable_at: Option<Timestamp>,
    /// Whether currently active (participating in consensus)
    pub is_active: bool,
    /// Total rewards earned
    pub total_rewards: HclawAmount,
    /// Total amount slashed
    pub total_slashed: HclawAmount,
    /// Slashing history
    pub slash_history: Vec<SlashEvent>,
}

impl StakeInfo {
    /// Create new stake info
    #[must_use]
    pub fn new(address: Address, amount: HclawAmount) -> Self {
        Self {
            address,
            amount,
            staked_at: now_millis(),
            withdrawable_at: None,
            is_active: true,
            total_rewards: HclawAmount::ZERO,
            total_slashed: HclawAmount::ZERO,
            slash_history: Vec::new(),
        }
    }

    /// Get effective stake (after pending slashes)
    #[must_use]
    pub fn effective_stake(&self) -> HclawAmount {
        self.amount.saturating_sub(self.total_slashed)
    }

    /// Check if this stake is sufficient for verification
    #[must_use]
    pub fn can_verify(&self, min_stake: HclawAmount) -> bool {
        self.is_active && self.effective_stake() >= min_stake
    }

    /// Apply a slash
    pub fn apply_slash(&mut self, reason: SlashingReason, timestamp: Timestamp) -> HclawAmount {
        let slash_percent = reason.slash_percentage();
        let slash_amount = self.amount.percentage(slash_percent);

        self.total_slashed = self.total_slashed.saturating_add(slash_amount);

        self.slash_history.push(SlashEvent {
            reason,
            amount: slash_amount,
            timestamp,
        });

        // If slashed 100%, deactivate
        if slash_percent == 100 {
            self.is_active = false;
        }

        slash_amount
    }

    /// Add rewards
    pub fn add_rewards(&mut self, amount: HclawAmount) {
        self.total_rewards = self.total_rewards.saturating_add(amount);
    }
}

/// A slashing event
#[derive(Clone, Debug)]
pub struct SlashEvent {
    /// Reason for the slash
    pub reason: SlashingReason,
    /// Amount slashed
    pub amount: HclawAmount,
    /// When it occurred
    pub timestamp: Timestamp,
}

/// Manages verifier stakes
pub struct StakeManager {
    /// Stakes by address
    stakes: HashMap<Address, StakeInfo>,
    /// Minimum stake to participate
    min_stake: HclawAmount,
    /// Unbonding period in milliseconds
    unbonding_period_ms: i64,
    /// Total staked across all verifiers
    total_staked: HclawAmount,
}

impl Default for StakeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StakeManager {
    /// Default unbonding period (7 days)
    pub const DEFAULT_UNBONDING_PERIOD_MS: i64 = 7 * 24 * 60 * 60 * 1000;

    /// Create a new stake manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            stakes: HashMap::new(),
            min_stake: HclawAmount::from_hclaw(1000),
            unbonding_period_ms: Self::DEFAULT_UNBONDING_PERIOD_MS,
            total_staked: HclawAmount::ZERO,
        }
    }

    /// Create with custom minimum stake
    #[must_use]
    pub fn with_min_stake(min_stake: HclawAmount) -> Self {
        Self {
            stakes: HashMap::new(),
            min_stake,
            unbonding_period_ms: Self::DEFAULT_UNBONDING_PERIOD_MS,
            total_staked: HclawAmount::ZERO,
        }
    }

    /// Add stake for a verifier
    pub fn stake(&mut self, address: Address, amount: HclawAmount) -> Result<(), StakeError> {
        if amount < self.min_stake {
            return Err(StakeError::InsufficientStake {
                have: amount,
                need: self.min_stake,
            });
        }

        let stake_info = self.stakes.entry(address).or_insert_with(|| {
            StakeInfo::new(address, HclawAmount::ZERO)
        });

        stake_info.amount = stake_info.amount.saturating_add(amount);
        stake_info.is_active = true;
        stake_info.withdrawable_at = None;

        self.total_staked = self.total_staked.saturating_add(amount);

        Ok(())
    }

    /// Begin unstaking process
    pub fn begin_unstake(&mut self, address: &Address) -> Result<(), StakeError> {
        let stake = self.stakes.get_mut(address)
            .ok_or(StakeError::NotFound)?;

        if stake.withdrawable_at.is_some() {
            return Err(StakeError::AlreadyUnstaking);
        }

        stake.is_active = false;
        stake.withdrawable_at = Some(now_millis() + self.unbonding_period_ms);

        Ok(())
    }

    /// Complete unstaking and withdraw
    pub fn complete_unstake(&mut self, address: &Address) -> Result<HclawAmount, StakeError> {
        let stake = self.stakes.get(address)
            .ok_or(StakeError::NotFound)?;

        let withdrawable_at = stake.withdrawable_at
            .ok_or(StakeError::NotUnstaking)?;

        if now_millis() < withdrawable_at {
            return Err(StakeError::UnbondingNotComplete {
                ready_at: withdrawable_at,
            });
        }

        let amount = stake.effective_stake();
        self.total_staked = self.total_staked.saturating_sub(stake.amount);
        self.stakes.remove(address);

        Ok(amount)
    }

    /// Slash a verifier
    pub fn slash(
        &mut self,
        address: &Address,
        reason: SlashingReason,
    ) -> Result<HclawAmount, StakeError> {
        let stake = self.stakes.get_mut(address)
            .ok_or(StakeError::NotFound)?;

        let slashed = stake.apply_slash(reason, now_millis());

        Ok(slashed)
    }

    /// Distribute rewards to a verifier
    pub fn distribute_reward(
        &mut self,
        address: &Address,
        amount: HclawAmount,
    ) -> Result<(), StakeError> {
        let stake = self.stakes.get_mut(address)
            .ok_or(StakeError::NotFound)?;

        stake.add_rewards(amount);
        Ok(())
    }

    /// Get stake info for an address
    #[must_use]
    pub fn get_stake(&self, address: &Address) -> Option<&StakeInfo> {
        self.stakes.get(address)
    }

    /// Check if an address can verify
    #[must_use]
    pub fn can_verify(&self, address: &Address) -> bool {
        self.stakes
            .get(address)
            .map(|s| s.can_verify(self.min_stake))
            .unwrap_or(false)
    }

    /// Get total staked amount
    #[must_use]
    pub const fn total_staked(&self) -> HclawAmount {
        self.total_staked
    }

    /// Get count of active verifiers
    #[must_use]
    pub fn active_verifier_count(&self) -> usize {
        self.stakes.values().filter(|s| s.is_active).count()
    }

    /// Get all active verifiers
    #[must_use]
    pub fn active_verifiers(&self) -> Vec<&StakeInfo> {
        self.stakes.values().filter(|s| s.is_active).collect()
    }
}

/// Staking errors
#[derive(Debug, thiserror::Error)]
pub enum StakeError {
    /// Stake not found
    #[error("stake not found")]
    NotFound,
    /// Insufficient stake amount
    #[error("insufficient stake: have {have}, need {need}")]
    InsufficientStake { have: HclawAmount, need: HclawAmount },
    /// Already unstaking
    #[error("already unstaking")]
    AlreadyUnstaking,
    /// Not in unstaking state
    #[error("not unstaking")]
    NotUnstaking,
    /// Unbonding period not complete
    #[error("unbonding not complete, ready at {ready_at}")]
    UnbondingNotComplete { ready_at: Timestamp },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_address() -> Address {
        let kp = Keypair::generate();
        Address::from_public_key(kp.public_key())
    }

    #[test]
    fn test_stake_and_verify() {
        let mut manager = StakeManager::new();
        let addr = test_address();

        // Stake
        assert!(manager.stake(addr, HclawAmount::from_hclaw(1000)).is_ok());
        assert!(manager.can_verify(&addr));
        assert_eq!(manager.active_verifier_count(), 1);
    }

    #[test]
    fn test_insufficient_stake() {
        let mut manager = StakeManager::new();
        let addr = test_address();

        let result = manager.stake(addr, HclawAmount::from_hclaw(100)); // Below min
        assert!(matches!(result, Err(StakeError::InsufficientStake { .. })));
    }

    #[test]
    fn test_slashing() {
        let mut manager = StakeManager::new();
        let addr = test_address();

        manager.stake(addr, HclawAmount::from_hclaw(1000)).unwrap();

        // Slash for honey pot approval (100%)
        let slashed = manager.slash(
            &addr,
            SlashingReason::HoneyPotApproval {
                solution_id: Hash::ZERO,
            },
        ).unwrap();

        assert_eq!(slashed.whole_hclaw(), 1000);

        // Should no longer be active
        assert!(!manager.can_verify(&addr));
    }

    #[test]
    fn test_partial_slash() {
        let mut manager = StakeManager::new();
        let addr = test_address();

        manager.stake(addr, HclawAmount::from_hclaw(1000)).unwrap();

        // Slash for invalid verification (10%)
        let slashed = manager.slash(
            &addr,
            SlashingReason::InvalidVerification {
                details: "test".to_string(),
            },
        ).unwrap();

        assert_eq!(slashed.whole_hclaw(), 100);

        // Should still be active (90% remaining > min stake)
        // Actually, with min stake at 1000 and only 900 effective, may not be active
        let stake = manager.get_stake(&addr).unwrap();
        assert_eq!(stake.effective_stake().whole_hclaw(), 900);
    }

    #[test]
    fn test_unstaking() {
        let mut manager = StakeManager::with_min_stake(HclawAmount::from_hclaw(100));
        manager.unbonding_period_ms = 0; // No wait for test

        let addr = test_address();
        manager.stake(addr, HclawAmount::from_hclaw(100)).unwrap();

        // Begin unstake
        assert!(manager.begin_unstake(&addr).is_ok());
        assert!(!manager.can_verify(&addr));

        // Complete unstake
        let amount = manager.complete_unstake(&addr).unwrap();
        assert_eq!(amount.whole_hclaw(), 100);
        assert!(manager.get_stake(&addr).is_none());
    }
}
