//! Verifier (Miner) implementation with security features.
//!
//! ## Security: Honey Pot Defense
//!
//! The Lazy Miner Attack: Miners approve every solution without checking to save CPU.
//!
//! Defense: The protocol injects valid-looking but *invalid* solutions (honey pots).
//! If a miner signs a honey pot, their entire stake is slashed.

mod honey_pot;
mod stake;

pub use honey_pot::{HoneyPotGenerator, HoneyPotDetector};
pub use stake::{StakeManager, SlashingReason, StakeInfo};


use crate::crypto::{Hash, Keypair, PublicKey};
use crate::types::{
    Address, Block, JobPacket, HclawAmount, SolutionCandidate, VerificationResult,
};
use crate::consensus::{BlockProducer, BlockProducerConfig};

/// Verifier node configuration
#[derive(Clone, Debug)]
pub struct VerifierConfig {
    /// Minimum stake required to verify
    pub min_stake: HclawAmount,
    /// Block production config
    pub block_config: BlockProducerConfig,
    /// Enable honey pot generation (for protocol operators)
    pub generate_honey_pots: bool,
    /// Honey pot injection rate (0.0 - 1.0)
    pub honey_pot_rate: f64,
}

impl Default for VerifierConfig {
    fn default() -> Self {
        Self {
            min_stake: HclawAmount::from_hclaw(1000),
            block_config: BlockProducerConfig::default(),
            generate_honey_pots: false,
            honey_pot_rate: 0.01, // 1% of solutions are honey pots
        }
    }
}

/// A verifier (miner) node
pub struct Verifier {
    /// Node keypair
    keypair: Keypair,
    /// Node address
    address: Address,
    /// Configuration
    config: VerifierConfig,
    /// Block producer
    block_producer: BlockProducer,
    /// Stake manager
    stake_manager: StakeManager,
    /// Honey pot generator (if enabled)
    honey_pot_generator: Option<HoneyPotGenerator>,
    /// Honey pot detector
    honey_pot_detector: HoneyPotDetector,
    /// Statistics
    stats: VerifierStats,
}

impl Verifier {
    /// Create a new verifier node
    #[must_use]
    pub fn new(keypair: Keypair, config: VerifierConfig) -> Self {
        let address = Address::from_public_key(keypair.public_key());

        let honey_pot_generator = if config.generate_honey_pots {
            Some(HoneyPotGenerator::new(config.honey_pot_rate))
        } else {
            None
        };

        Self {
            block_producer: BlockProducer::new(
                Keypair::from_secret(
                    crate::crypto::SecretKey::from_bytes(keypair.public_key().as_bytes().clone())
                        .unwrap_or_else(|_| crate::crypto::SecretKey::generate())
                ),
                config.block_config.clone(),
            ),
            address,
            config,
            keypair,
            stake_manager: StakeManager::new(),
            honey_pot_generator,
            honey_pot_detector: HoneyPotDetector::new(),
            stats: VerifierStats::default(),
        }
    }

    /// Get verifier's address
    #[must_use]
    pub const fn address(&self) -> &Address {
        &self.address
    }

    /// Get verifier's public key
    #[must_use]
    pub fn public_key(&self) -> &PublicKey {
        self.keypair.public_key()
    }

    /// Process a solution candidate
    ///
    /// Returns the verification result and whether this was a honey pot.
    pub fn process_solution(
        &mut self,
        job: &JobPacket,
        solution: &SolutionCandidate,
    ) -> Result<(VerificationResult, bool), VerifierError> {
        self.stats.solutions_processed += 1;

        // Check if this is a known honey pot
        let is_honey_pot = self.honey_pot_detector.is_honey_pot(&solution.id);

        // Perform actual verification
        let result = self.block_producer.verify_solution(job, solution)
            .map_err(|e| VerifierError::VerificationFailed(e.to_string()))?;

        if result.passed {
            self.stats.solutions_verified += 1;
        } else {
            self.stats.solutions_rejected += 1;

            // If we correctly rejected a honey pot, good job!
            if is_honey_pot {
                self.stats.honey_pots_caught += 1;
            }
        }

        Ok((result, is_honey_pot))
    }

    /// Generate a honey pot solution for a job
    ///
    /// Only available if honey pot generation is enabled.
    pub fn generate_honey_pot(&mut self, job: &JobPacket) -> Option<SolutionCandidate> {
        let generator = self.honey_pot_generator.as_mut()?;

        let honey_pot = generator.generate(job, self.keypair.public_key());

        // Register it with the detector
        self.honey_pot_detector.register(&honey_pot.id);

        Some(honey_pot)
    }

    /// Check if a verifier approved a honey pot (should be slashed)
    pub fn check_for_honey_pot_approval(
        &self,
        _verifier: &PublicKey,
        approved_solutions: &[Hash],
    ) -> Option<Hash> {
        for solution_id in approved_solutions {
            if self.honey_pot_detector.is_honey_pot(solution_id) {
                return Some(*solution_id);
            }
        }
        None
    }

    /// Try to produce a block if ready
    pub fn try_produce_block(&mut self, state_root: Hash) -> Result<Option<Block>, VerifierError> {
        if !self.block_producer.should_produce_block() {
            return Ok(None);
        }

        let block = self.block_producer.produce_block(state_root)
            .map_err(|e| VerifierError::BlockProductionFailed(e.to_string()))?;

        self.stats.blocks_produced += 1;

        Ok(Some(block))
    }

    /// Get verifier statistics
    #[must_use]
    pub const fn stats(&self) -> &VerifierStats {
        &self.stats
    }

    /// Get stake info for an address
    #[must_use]
    pub fn get_stake(&self, address: &Address) -> Option<&StakeInfo> {
        self.stake_manager.get_stake(address)
    }
}

/// Verifier statistics
#[derive(Clone, Debug, Default)]
pub struct VerifierStats {
    /// Total solutions processed
    pub solutions_processed: u64,
    /// Solutions that passed verification
    pub solutions_verified: u64,
    /// Solutions that failed verification
    pub solutions_rejected: u64,
    /// Honey pots correctly caught
    pub honey_pots_caught: u64,
    /// Blocks produced
    pub blocks_produced: u64,
    /// Total rewards earned
    pub total_rewards: HclawAmount,
}

/// Verifier errors
#[derive(Debug, thiserror::Error)]
pub enum VerifierError {
    /// Verification failed
    #[error("verification failed: {0}")]
    VerificationFailed(String),
    /// Insufficient stake
    #[error("insufficient stake: have {have}, need {need}")]
    InsufficientStake {
        /// Amount of stake the verifier has
        have: HclawAmount,
        /// Amount of stake required
        need: HclawAmount,
    },
    /// Block production failed
    #[error("block production failed: {0}")]
    BlockProductionFailed(String),
    /// Verifier is slashed
    #[error("verifier is slashed: {reason}")]
    Slashed {
        /// Reason for slashing
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{JobType, VerificationSpec};
    use crate::crypto::hash_data;

    fn create_test_verifier() -> Verifier {
        let kp = Keypair::generate();
        Verifier::new(kp, VerifierConfig::default())
    }

    fn create_test_job_solution() -> (JobPacket, SolutionCandidate) {
        let requester_kp = Keypair::generate();
        let solver_kp = Keypair::generate();

        let output = b"correct output";
        let expected_hash = hash_data(output);

        let mut job = JobPacket::new(
            JobType::Deterministic,
            *requester_kp.public_key(),
            b"input".to_vec(),
            "Test".to_string(),
            HclawAmount::from_hclaw(10),
            HclawAmount::from_hclaw(1),
            VerificationSpec::HashMatch { expected_hash },
            3600,
        );
        job.signature = requester_kp.sign(&job.signing_bytes());

        let mut solution = SolutionCandidate::new(
            job.id,
            *solver_kp.public_key(),
            output.to_vec(),
        );
        solution.signature = solver_kp.sign(&solution.signing_bytes());

        (job, solution)
    }

    #[test]
    fn test_verifier_creation() {
        let verifier = create_test_verifier();
        assert_eq!(verifier.stats().solutions_processed, 0);
    }

    #[test]
    fn test_process_valid_solution() {
        let mut verifier = create_test_verifier();
        let (job, solution) = create_test_job_solution();

        let (result, is_honey_pot) = verifier.process_solution(&job, &solution).unwrap();

        assert!(result.passed);
        assert!(!is_honey_pot);
        assert_eq!(verifier.stats().solutions_verified, 1);
    }
}
