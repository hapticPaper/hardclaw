//! Solution Candidates - submitted by Solvers after completing work.

use serde::{Deserialize, Serialize};

use crate::crypto::{hash_data, Hash, PublicKey, Signature};
use super::{Address, Id, Timestamp, now_millis};

/// Status of a solution candidate
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SolutionStatus {
    /// Submitted, awaiting verification
    Pending,
    /// Currently being verified
    Verifying,
    /// Verified as correct
    Verified,
    /// Rejected as incorrect
    Rejected,
    /// Identified as a honey pot test (for lazy miner detection)
    HoneyPot,
}

/// A solution submitted by a Solver
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolutionCandidate {
    /// Unique solution ID
    pub id: Id,
    /// The job this solves
    pub job_id: Id,
    /// Solver's public key
    pub solver: PublicKey,
    /// Solver's address (for bounty payment)
    pub solver_address: Address,
    /// The solution output (opaque bytes)
    pub output: Vec<u8>,
    /// Hash of the output (for quick comparison)
    pub output_hash: Hash,
    /// When the solution was submitted
    pub submitted_at: Timestamp,
    /// Solver's signature
    pub signature: Signature,
    /// Current status
    pub status: SolutionStatus,
    /// Whether this is a honey pot (only known to protocol)
    #[serde(skip)]
    pub is_honey_pot: bool,
}

impl SolutionCandidate {
    /// Create a new solution candidate (unsigned)
    #[must_use]
    pub fn new(job_id: Id, solver: PublicKey, output: Vec<u8>) -> Self {
        let output_hash = hash_data(&output);
        let solver_address = Address::from_public_key(&solver);
        let submitted_at = now_millis();

        let mut solution = Self {
            id: Hash::ZERO,
            job_id,
            solver,
            solver_address,
            output,
            output_hash,
            submitted_at,
            signature: Signature::from_bytes([0u8; 64]),
            status: SolutionStatus::Pending,
            is_honey_pot: false,
        };

        solution.id = solution.compute_id();
        solution
    }

    /// Create a honey pot solution (for lazy miner detection)
    ///
    /// These look valid but have deliberately wrong outputs.
    #[must_use]
    pub fn create_honey_pot(job_id: Id, solver: PublicKey, fake_output: Vec<u8>) -> Self {
        let mut solution = Self::new(job_id, solver, fake_output);
        solution.is_honey_pot = true;
        solution
    }

    /// Compute the solution ID
    #[must_use]
    pub fn compute_id(&self) -> Id {
        let mut data = Vec::new();
        data.extend_from_slice(self.job_id.as_bytes());
        data.extend_from_slice(self.solver.as_bytes());
        data.extend_from_slice(self.output_hash.as_bytes());
        data.extend_from_slice(&self.submitted_at.to_le_bytes());

        hash_data(&data)
    }

    /// Get the bytes to sign
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.id.as_bytes());
        data.extend_from_slice(self.job_id.as_bytes());
        data.extend_from_slice(self.solver.as_bytes());
        data.extend_from_slice(self.output_hash.as_bytes());
        data.extend_from_slice(&self.submitted_at.to_le_bytes());
        data
    }

    /// Verify the solution signature
    ///
    /// # Errors
    /// Returns error if signature is invalid
    pub fn verify_signature(&self) -> Result<(), crate::crypto::CryptoError> {
        crate::crypto::verify(&self.solver, &self.signing_bytes(), &self.signature)
    }

    /// Check if this solution is pending verification
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        matches!(self.status, SolutionStatus::Pending)
    }

    /// Check if this solution was verified as correct
    #[must_use]
    pub const fn is_verified(&self) -> bool {
        matches!(self.status, SolutionStatus::Verified)
    }

    /// Mark as verified
    pub fn mark_verified(&mut self) {
        self.status = SolutionStatus::Verified;
    }

    /// Mark as rejected
    pub fn mark_rejected(&mut self) {
        self.status = SolutionStatus::Rejected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_solution_creation() {
        let kp = Keypair::generate();
        let job_id = hash_data(b"test job");

        let solution = SolutionCandidate::new(
            job_id,
            *kp.public_key(),
            b"solution output".to_vec(),
        );

        assert_eq!(solution.job_id, job_id);
        assert_eq!(solution.status, SolutionStatus::Pending);
        assert!(!solution.is_honey_pot);
    }

    #[test]
    fn test_honey_pot() {
        let kp = Keypair::generate();
        let job_id = hash_data(b"test job");

        let honey_pot = SolutionCandidate::create_honey_pot(
            job_id,
            *kp.public_key(),
            b"fake output".to_vec(),
        );

        assert!(honey_pot.is_honey_pot);
    }

    #[test]
    fn test_solution_signature() {
        let kp = Keypair::generate();
        let job_id = hash_data(b"test job");

        let mut solution = SolutionCandidate::new(
            job_id,
            *kp.public_key(),
            b"output".to_vec(),
        );

        solution.signature = kp.sign(&solution.signing_bytes());
        assert!(solution.verify_signature().is_ok());
    }
}
