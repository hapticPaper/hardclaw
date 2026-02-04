//! Proof-of-Verification Consensus Mechanism.
//!
//! Unlike Proof-of-Work (which wastes energy) or Proof-of-Stake (which centralizes wealth),
//! PoV "mines" blocks by cryptographically verifying useful work performed by off-chain agents.
//!
//! ## The Asymmetry Principle
//!
//! The protocol relies on computational asymmetry:
//! - **Generation (P_gen)**: High complexity (inference, optimization, rendering)
//! - **Verification (P_ver)**: Low complexity (hash check, physics sim, deterministic logic)
//!
//! ## Consensus Flow
//!
//! 1. Miner grabs a Solution Candidate from the pool
//! 2. Miner runs the deterministic `Verify()` function
//! 3. If True, Miner signs the solution and adds it to the candidate block
//! 4. Block is valid only if 66% of network agrees verifications are correct

mod pov;
mod block_producer;

pub use pov::ProofOfVerification;
pub use block_producer::{BlockProducer, BlockProducerConfig};

use thiserror::Error;

use crate::types::{Block, SolutionCandidate, JobPacket, VerificationResult};

/// Consensus errors
#[derive(Debug, Error)]
pub enum ConsensusError {
    /// Block doesn't meet consensus threshold
    #[error("insufficient consensus: {percentage:.1}% < 66%")]
    InsufficientConsensus {
        /// Percentage of verifiers who attested
        percentage: f64,
    },

    /// Invalid parent block reference
    #[error("invalid parent block")]
    InvalidParent,

    /// Block height mismatch
    #[error("block height mismatch: expected {expected}, got {got}")]
    HeightMismatch {
        /// Expected block height
        expected: u64,
        /// Actual block height received
        got: u64,
    },

    /// Verification failed
    #[error("verification failed: {reason}")]
    VerificationFailed {
        /// Failure reason
        reason: String,
    },

    /// Block already exists
    #[error("block already exists at height {height}")]
    BlockExists {
        /// Block height that already exists
        height: u64,
    },

    /// Missing job for solution
    #[error("job not found: {job_id}")]
    JobNotFound {
        /// The job ID that was not found
        job_id: String,
    },

    /// Solution doesn't match job
    #[error("solution doesn't match job specification")]
    SolutionMismatch,
}

/// Trait for verifying solutions
pub trait SolutionVerifier: Send + Sync {
    /// Verify a solution against its job specification
    fn verify(
        &self,
        job: &JobPacket,
        solution: &SolutionCandidate,
    ) -> Result<VerificationResult, ConsensusError>;
}

/// Trait for consensus participation
pub trait ConsensusParticipant: Send + Sync {
    /// Get the current chain tip
    fn chain_tip(&self) -> Option<&Block>;

    /// Get block by hash
    fn get_block(&self, hash: &crate::crypto::Hash) -> Option<&Block>;

    /// Get block by height
    fn get_block_by_height(&self, height: u64) -> Option<&Block>;

    /// Add a new block to the chain
    fn add_block(&mut self, block: Block) -> Result<(), ConsensusError>;

    /// Get current difficulty/target
    fn current_difficulty(&self) -> u64;

    /// Get total number of active verifiers
    fn active_verifier_count(&self) -> usize;
}
