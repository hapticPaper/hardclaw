//! Block production for Proof-of-Verification.
//!
//! Verifiers assemble blocks by:
//! 1. Pulling solutions from the mempool
//! 2. Running verification on each
//! 3. Creating a block with verified solutions
//! 4. Broadcasting for attestations

use std::collections::VecDeque;

use crate::crypto::{Hash, Keypair};
use crate::types::{Block, HclawAmount, JobPacket, SolutionCandidate, VerificationResult};

use super::{ConsensusError, ProofOfVerification};

/// Configuration for block production
#[derive(Clone, Debug)]
pub struct BlockProducerConfig {
    /// Maximum solutions per block
    pub max_solutions_per_block: usize,
    /// Maximum block size in bytes
    pub max_block_size: usize,
    /// Target block time in milliseconds
    pub target_block_time_ms: u64,
    /// Minimum verifications needed to produce block
    pub min_verifications: usize,
}

impl Default for BlockProducerConfig {
    fn default() -> Self {
        Self {
            max_solutions_per_block: 1000,
            max_block_size: 1_000_000,  // 1 MB
            target_block_time_ms: 1000, // 1 second target
            min_verifications: 1,
        }
    }
}

/// Block producer (verifier/miner role)
pub struct BlockProducer {
    /// Configuration
    config: BlockProducerConfig,
    /// Our keypair for signing
    keypair: Keypair,
    /// `PoV` consensus engine
    pov: ProofOfVerification,
    /// Pending verifications for current block
    pending_verifications: VecDeque<VerificationResult>,
    /// Current chain height
    current_height: u64,
    /// Current parent hash
    current_parent: Hash,
}

impl BlockProducer {
    /// Create a new block producer
    #[must_use]
    pub fn new(keypair: Keypair, config: BlockProducerConfig) -> Self {
        Self {
            config,
            keypair,
            pov: ProofOfVerification::new(),
            pending_verifications: VecDeque::new(),
            current_height: 0,
            current_parent: Hash::ZERO,
        }
    }

    /// Set the current chain state
    pub fn set_chain_state(&mut self, height: u64, parent_hash: Hash) {
        self.current_height = height;
        self.current_parent = parent_hash;
    }

    /// Process a solution candidate
    ///
    /// Returns the verification result if successful.
    pub fn verify_solution(
        &mut self,
        job: &JobPacket,
        solution: &SolutionCandidate,
    ) -> Result<VerificationResult, ConsensusError> {
        let result = self.pov.verify_solution(job, solution, &self.keypair)?;

        // Only add passed verifications to pending
        if result.passed {
            self.pending_verifications.push_back(result.clone());
        }

        Ok(result)
    }

    /// Check if we should produce a block
    #[must_use]
    pub fn should_produce_block(&self) -> bool {
        self.pending_verifications.len() >= self.config.min_verifications
    }

    /// Produce a new block from pending verifications
    pub fn produce_block(&mut self, state_root: Hash) -> Result<Block, ConsensusError> {
        if self.pending_verifications.is_empty() {
            return Err(ConsensusError::VerificationFailed {
                reason: "No verifications to include in block".to_string(),
            });
        }

        // Take up to max_solutions_per_block verifications
        let mut verifications = Vec::new();
        let mut total_size = 0;

        while let Some(verification) = self.pending_verifications.pop_front() {
            // Estimate size (rough approximation)
            let estimated_size = 256; // Approximate size per verification

            if verifications.len() >= self.config.max_solutions_per_block
                || total_size + estimated_size > self.config.max_block_size
            {
                // Put it back and stop
                self.pending_verifications.push_front(verification);
                break;
            }

            verifications.push(verification);
            total_size += estimated_size;
        }

        // Create the block
        let mut block = Block::new(
            self.current_height + 1,
            self.current_parent,
            self.keypair.public_key().clone(),
            verifications,
            state_root,
        );

        // Sign the block
        block.proposer_signature = self.keypair.sign(&block.signing_bytes());

        // Create our own attestation
        let verified_solutions: Vec<Hash> =
            block.verifications.iter().map(|v| v.solution_id).collect();

        let attestation = self
            .pov
            .create_attestation(&block, verified_solutions, &self.keypair);
        block.add_attestation(attestation);

        Ok(block)
    }

    /// Get the number of pending verifications
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending_verifications.len()
    }

    /// Clear pending verifications (e.g., after chain reorg)
    pub fn clear_pending(&mut self) {
        self.pending_verifications.clear();
    }

    /// Get our public key
    #[must_use]
    pub fn public_key(&self) -> &crate::crypto::PublicKey {
        self.keypair.public_key()
    }
}

/// Statistics about block production
#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct BlockProducerStats {
    /// Total blocks produced
    pub blocks_produced: u64,
    /// Total solutions verified
    pub solutions_verified: u64,
    /// Total solutions passed verification
    pub solutions_passed: u64,
    /// Total solutions failed verification
    pub solutions_failed: u64,
    /// Total rewards earned (in HCLAW)
    pub total_rewards: HclawAmount,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::hash_data;
    use crate::types::{JobType, VerificationSpec};

    fn create_test_job_solution() -> (JobPacket, SolutionCandidate) {
        let requester_kp = Keypair::generate();
        let solver_kp = Keypair::generate();

        let output = b"test output";
        let expected_hash = hash_data(output);

        let mut job = JobPacket::new(
            JobType::Deterministic,
            requester_kp.public_key().clone(),
            b"input".to_vec(),
            "Test".to_string(),
            HclawAmount::from_hclaw(10),
            HclawAmount::from_hclaw(1),
            VerificationSpec::HashMatch { expected_hash },
            3600,
        );
        job.signature = requester_kp.sign(&job.signing_bytes());

        let mut solution =
            SolutionCandidate::new(job.id, solver_kp.public_key().clone(), output.to_vec());
        solution.signature = solver_kp.sign(&solution.signing_bytes());

        (job, solution)
    }

    #[test]
    fn test_block_producer_creation() {
        let kp = Keypair::generate();
        let producer = BlockProducer::new(kp, BlockProducerConfig::default());

        assert_eq!(producer.pending_count(), 0);
        assert!(!producer.should_produce_block());
    }

    #[test]
    fn test_verify_and_produce() {
        let kp = Keypair::generate();
        let mut producer = BlockProducer::new(kp, BlockProducerConfig::default());

        let (job, solution) = create_test_job_solution();

        // Verify the solution
        let result = producer.verify_solution(&job, &solution).unwrap();
        assert!(result.passed);
        assert_eq!(producer.pending_count(), 1);

        // Should now be ready to produce
        assert!(producer.should_produce_block());

        // Produce block
        let block = producer.produce_block(Hash::ZERO).unwrap();
        assert_eq!(block.header.height, 1);
        assert_eq!(block.verifications.len(), 1);
        assert!(!block.attestations.is_empty());
    }

    #[test]
    fn test_failed_verification_not_added() {
        let kp = Keypair::generate();
        let mut producer = BlockProducer::new(kp, BlockProducerConfig::default());

        let (job, _) = create_test_job_solution();

        // Create bad solution
        let solver_kp = Keypair::generate();
        let mut bad_solution = SolutionCandidate::new(
            job.id,
            solver_kp.public_key().clone(),
            b"wrong output".to_vec(),
        );
        bad_solution.signature = solver_kp.sign(&bad_solution.signing_bytes());

        // Verify - should succeed but not pass
        let result = producer.verify_solution(&job, &bad_solution).unwrap();
        assert!(!result.passed);

        // Should not be added to pending
        assert_eq!(producer.pending_count(), 0);
    }
}
