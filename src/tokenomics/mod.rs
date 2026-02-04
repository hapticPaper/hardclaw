//! $HCLAW Tokenomics
//!
//! $HCLAW is not a store of value; it is a **Store of Compute**.
//!
//! ## Key Properties
//!
//! - 1 HCLAW ≈ Market rate for 1 GigaFLOP of verified compute
//! - Supply is elastic: As demand rises, mining difficulty adjusts
//!
//! ## Fee Structure
//!
//! - 95% goes to Solver (the agent doing the work)
//! - 4% goes to Verifier (the miner securing the chain)
//! - 1% is burned to offset state bloat

mod distribution;
mod burn;
mod supply;

pub use distribution::{FeeDistributor, FeeDistribution};
pub use burn::{BurnManager, BurnReason};
pub use supply::{SupplyManager, SupplyMetrics};

use crate::types::{Address, HclawAmount};

/// Token economics configuration
#[derive(Clone, Debug)]
pub struct TokenEconomicsConfig {
    /// Percentage to solver (0-100)
    pub solver_share: u8,
    /// Percentage to verifier (0-100)
    pub verifier_share: u8,
    /// Percentage to burn (0-100)
    pub burn_share: u8,
    /// Minimum burn for job submission (anti-Sybil)
    pub min_burn_to_request: HclawAmount,
    /// Target block reward (adjusted by difficulty)
    pub target_block_reward: HclawAmount,
}

impl Default for TokenEconomicsConfig {
    fn default() -> Self {
        Self {
            solver_share: 95,
            verifier_share: 4,
            burn_share: 1,
            min_burn_to_request: HclawAmount::from_raw(1_000_000_000_000_000), // 0.001 HCLAW
            target_block_reward: HclawAmount::from_hclaw(10),
        }
    }
}

impl TokenEconomicsConfig {
    /// Validate that shares sum to 100
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.solver_share + self.verifier_share + self.burn_share == 100
    }
}

/// Main token economics engine
pub struct TokenEconomics {
    /// Configuration
    config: TokenEconomicsConfig,
    /// Fee distributor
    fee_distributor: FeeDistributor,
    /// Burn manager
    burn_manager: BurnManager,
    /// Supply manager
    supply_manager: SupplyManager,
}

impl Default for TokenEconomics {
    fn default() -> Self {
        Self::new(TokenEconomicsConfig::default())
    }
}

impl TokenEconomics {
    /// Create new token economics engine
    #[must_use]
    pub fn new(config: TokenEconomicsConfig) -> Self {
        assert!(config.is_valid(), "Fee shares must sum to 100");

        Self {
            fee_distributor: FeeDistributor::new(
                config.solver_share,
                config.verifier_share,
                config.burn_share,
            ),
            burn_manager: BurnManager::new(),
            supply_manager: SupplyManager::new(),
            config,
        }
    }

    /// Process a completed job and distribute fees
    pub fn process_job_completion(
        &mut self,
        bounty: HclawAmount,
        solver: Address,
        verifier: Address,
    ) -> FeeDistribution {
        let distribution = self.fee_distributor.distribute(bounty, solver, verifier);

        // Record the burn
        self.burn_manager.burn(distribution.burn_amount, BurnReason::JobFee);

        distribution
    }

    /// Process burn-to-request for job submission
    pub fn process_job_submission(&mut self, burn_amount: HclawAmount) -> Result<(), TokenError> {
        if burn_amount < self.config.min_burn_to_request {
            return Err(TokenError::InsufficientBurn {
                required: self.config.min_burn_to_request,
                provided: burn_amount,
            });
        }

        self.burn_manager.burn(burn_amount, BurnReason::JobSubmission);
        Ok(())
    }

    /// Calculate block reward based on current difficulty
    #[must_use]
    pub fn calculate_block_reward(&self, difficulty: u64) -> HclawAmount {
        // Elastic supply: reward adjusts inversely with difficulty
        // Higher difficulty = more demand = lower reward per block
        if difficulty == 0 {
            return self.config.target_block_reward;
        }

        let base = self.config.target_block_reward.raw();
        let adjusted = base * 1000 / (1000 + difficulty as u128);

        HclawAmount::from_raw(adjusted.max(1))
    }

    /// Get current supply metrics
    #[must_use]
    pub fn supply_metrics(&self) -> &SupplyMetrics {
        self.supply_manager.metrics()
    }

    /// Get total burned
    #[must_use]
    pub fn total_burned(&self) -> HclawAmount {
        self.burn_manager.total_burned()
    }

    /// Get configuration
    #[must_use]
    pub const fn config(&self) -> &TokenEconomicsConfig {
        &self.config
    }
}

/// Token economics errors
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// Insufficient burn amount
    #[error("insufficient burn: required {required}, provided {provided}")]
    InsufficientBurn {
        /// Minimum burn amount required
        required: HclawAmount,
        /// Amount actually provided
        provided: HclawAmount,
    },
    /// Insufficient balance
    #[error("insufficient balance: have {have}, need {need}")]
    InsufficientBalance {
        /// Current balance
        have: HclawAmount,
        /// Amount needed
        need: HclawAmount,
    },
    /// Invalid distribution configuration
    #[error("invalid distribution: shares must sum to 100")]
    InvalidDistribution,
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
    fn test_fee_distribution() {
        let mut economics = TokenEconomics::default();

        let bounty = HclawAmount::from_hclaw(100);
        let solver = test_address();
        let verifier = test_address();

        let distribution = economics.process_job_completion(bounty, solver, verifier);

        assert_eq!(distribution.solver_amount.whole_hclaw(), 95);
        assert_eq!(distribution.verifier_amount.whole_hclaw(), 4);
        assert_eq!(distribution.burn_amount.whole_hclaw(), 1);
    }

    #[test]
    fn test_burn_to_request() {
        let mut economics = TokenEconomics::default();

        // Insufficient burn should fail
        let small_burn = HclawAmount::from_raw(100);
        assert!(economics.process_job_submission(small_burn).is_err());

        // Sufficient burn should succeed
        let good_burn = HclawAmount::from_hclaw(1);
        assert!(economics.process_job_submission(good_burn).is_ok());
    }

    #[test]
    fn test_elastic_block_reward() {
        let economics = TokenEconomics::default();

        // Base difficulty = base reward
        let reward0 = economics.calculate_block_reward(0);
        assert_eq!(reward0.whole_hclaw(), 10);

        // Higher difficulty = lower reward
        let reward1000 = economics.calculate_block_reward(1000);
        assert!(reward1000 < reward0);
    }
}
