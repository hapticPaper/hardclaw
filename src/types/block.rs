//! Blocks in the `HardClaw` blockchain.
//!
//! A block contains verified solutions and state transitions.
//! Blocks are valid only with 66% consensus from verifiers.

use serde::{Deserialize, Serialize};

use super::{now_millis, Id, Timestamp, VerificationResult};
use crate::crypto::{hash_data, merkle_root, Hash, PublicKey, Signature};

/// Block header containing metadata and commitments
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Block number (height)
    pub height: u64,
    /// Hash of the previous block
    pub parent_hash: Hash,
    /// Merkle root of verified solutions in this block
    pub solutions_root: Hash,
    /// Merkle root of state transitions
    pub state_root: Hash,
    /// Timestamp of block creation
    pub timestamp: Timestamp,
    /// Proposer's public key (the verifier who assembled this block)
    pub proposer: PublicKey,
    /// Number of verifications in this block
    pub verification_count: u32,
    /// Protocol version
    pub version: u32,
}

impl BlockHeader {
    /// Compute the block hash
    #[must_use]
    pub fn compute_hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(&self.height.to_le_bytes());
        data.extend_from_slice(self.parent_hash.as_bytes());
        data.extend_from_slice(self.solutions_root.as_bytes());
        data.extend_from_slice(self.state_root.as_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(self.proposer.as_bytes());
        data.extend_from_slice(&self.verification_count.to_le_bytes());
        data.extend_from_slice(&self.version.to_le_bytes());

        hash_data(&data)
    }
}

/// A verifier's attestation that they verified solutions in this block
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifierAttestation {
    /// The verifier's public key
    pub verifier: PublicKey,
    /// Hash of the block being attested
    pub block_hash: Hash,
    /// List of solution IDs this verifier verified
    pub verified_solutions: Vec<Id>,
    /// Signature over the attestation
    pub signature: Signature,
}

impl VerifierAttestation {
    /// Create a new attestation
    #[must_use]
    pub fn new(verifier: PublicKey, block_hash: Hash, verified_solutions: Vec<Id>) -> Self {
        Self {
            verifier,
            block_hash,
            verified_solutions,
            signature: Signature::from_bytes([0u8; 64]),
        }
    }

    /// Get bytes to sign
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.verifier.as_bytes());
        data.extend_from_slice(self.block_hash.as_bytes());

        for sol_id in &self.verified_solutions {
            data.extend_from_slice(sol_id.as_bytes());
        }

        data
    }

    /// Verify the attestation signature
    ///
    /// # Errors
    /// Returns error if signature is invalid
    pub fn verify_signature(&self) -> Result<(), crate::crypto::CryptoError> {
        crate::crypto::verify(&self.verifier, &self.signing_bytes(), &self.signature)
    }
}

/// A complete block in the `HardClaw` blockchain
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    /// Block header
    pub header: BlockHeader,
    /// Block hash (computed from header)
    pub hash: Hash,
    /// Verified solutions included in this block
    pub verifications: Vec<VerificationResult>,
    /// Attestations from verifiers (must have 66%+ agreement)
    pub attestations: Vec<VerifierAttestation>,
    /// Proposer's signature over the block
    pub proposer_signature: Signature,
}

impl Block {
    /// Create a new block
    #[must_use]
    pub fn new(
        height: u64,
        parent_hash: Hash,
        proposer: PublicKey,
        verifications: Vec<VerificationResult>,
        state_root: Hash,
    ) -> Self {
        let solutions_root = Self::compute_solutions_root(&verifications);
        let timestamp = now_millis();

        let header = BlockHeader {
            height,
            parent_hash,
            solutions_root,
            state_root,
            timestamp,
            proposer,
            verification_count: verifications.len() as u32,
            version: 1,
        };

        let hash = header.compute_hash();

        Self {
            header,
            hash,
            verifications,
            attestations: Vec::new(),
            proposer_signature: Signature::from_bytes([0u8; 64]),
        }
    }

    /// Create the genesis block
    #[must_use]
    pub fn genesis(proposer: PublicKey) -> Self {
        Self::new(0, Hash::ZERO, proposer, Vec::new(), Hash::ZERO)
    }

    /// Compute the merkle root of solutions
    fn compute_solutions_root(verifications: &[VerificationResult]) -> Hash {
        let hashes: Vec<Hash> = verifications.iter().map(|v| v.solution_id).collect();

        merkle_root(&hashes)
    }

    /// Add an attestation from a verifier
    pub fn add_attestation(&mut self, attestation: VerifierAttestation) {
        self.attestations.push(attestation);
    }

    /// Check if the block has reached consensus (66%+ attestations)
    ///
    /// # Arguments
    /// * `total_verifiers` - Total number of active verifiers in the network
    #[must_use]
    pub fn has_consensus(&self, total_verifiers: usize) -> bool {
        if total_verifiers == 0 {
            return false;
        }

        let threshold = (total_verifiers as f64 * crate::CONSENSUS_THRESHOLD).ceil() as usize;
        self.attestations.len() >= threshold
    }

    /// Get consensus percentage
    #[must_use]
    pub fn consensus_percentage(&self, total_verifiers: usize) -> f64 {
        if total_verifiers == 0 {
            return 0.0;
        }

        self.attestations.len() as f64 / total_verifiers as f64
    }

    /// Verify block integrity
    ///
    /// # Errors
    /// Returns error if block is invalid
    pub fn verify_integrity(&self) -> Result<(), BlockError> {
        // Check hash matches header
        let computed_hash = self.header.compute_hash();
        if computed_hash != self.hash {
            return Err(BlockError::HashMismatch);
        }

        // Check solutions root
        let computed_root = Self::compute_solutions_root(&self.verifications);
        if computed_root != self.header.solutions_root {
            return Err(BlockError::SolutionsRootMismatch);
        }

        // Verify attestation signatures
        for attestation in &self.attestations {
            attestation
                .verify_signature()
                .map_err(|_| BlockError::InvalidAttestation)?;
        }

        Ok(())
    }

    /// Get bytes to sign (for proposer signature)
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.hash.as_bytes());

        for v in &self.verifications {
            data.extend_from_slice(v.solution_id.as_bytes());
        }

        data
    }
}

/// Block validation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum BlockError {
    /// Block hash doesn't match header
    #[error("block hash mismatch")]
    HashMismatch,
    /// Solutions merkle root mismatch
    #[error("solutions root mismatch")]
    SolutionsRootMismatch,
    /// Invalid parent reference
    #[error("invalid parent hash")]
    InvalidParent,
    /// Block height mismatch
    #[error("invalid block height: expected {expected}, got {got}")]
    InvalidHeight { expected: u64, got: u64 },
    /// Insufficient consensus
    #[error("insufficient consensus: {percentage}% < 66%")]
    InsufficientConsensus { percentage: f64 },
    /// Invalid attestation signature
    #[error("invalid attestation signature")]
    InvalidAttestation,
    /// Block timestamp too far in future
    #[error("block timestamp in future")]
    FutureTimestamp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_genesis_block() {
        let kp = Keypair::generate();
        let genesis = Block::genesis(*kp.public_key());

        assert_eq!(genesis.header.height, 0);
        assert_eq!(genesis.header.parent_hash, Hash::ZERO);
    }

    #[test]
    fn test_block_hash_deterministic() {
        let kp = Keypair::generate();
        let block = Block::new(1, Hash::ZERO, *kp.public_key(), Vec::new(), Hash::ZERO);

        let computed = block.header.compute_hash();
        assert_eq!(computed, block.hash);
    }

    #[test]
    fn test_consensus_threshold() {
        let kp = Keypair::generate();
        let mut block = Block::new(1, Hash::ZERO, *kp.public_key(), Vec::new(), Hash::ZERO);

        // With 10 verifiers, need 7 (66% rounded up)
        assert!(!block.has_consensus(10));

        // Add 7 attestations
        for _ in 0..7 {
            let verifier_kp = Keypair::generate();
            let mut attestation =
                VerifierAttestation::new(*verifier_kp.public_key(), block.hash, Vec::new());
            attestation.signature = verifier_kp.sign(&attestation.signing_bytes());
            block.add_attestation(attestation);
        }

        assert!(block.has_consensus(10));
    }

    #[test]
    fn test_block_integrity() {
        let kp = Keypair::generate();
        let block = Block::new(1, Hash::ZERO, *kp.public_key(), Vec::new(), Hash::ZERO);

        assert!(block.verify_integrity().is_ok());
    }
}
