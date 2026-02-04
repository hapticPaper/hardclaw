//! Proof-of-Verification consensus implementation.
//!
//! "We do not trust; we verify."

use std::collections::HashMap;
use std::time::Instant;

use crate::crypto::{hash_data, Hash, Keypair};
use crate::types::{
    Block, JobPacket, SolutionCandidate,
    VerificationResult, VerificationSpec, VerifierAttestation, now_millis,
};

use super::{ConsensusError, SolutionVerifier};

/// Proof-of-Verification consensus engine
pub struct ProofOfVerification {
    /// Verification results cache
    verification_cache: HashMap<Hash, VerificationResult>,
    /// Maximum age for cached results (in milliseconds)
    cache_ttl_ms: i64,
}

impl Default for ProofOfVerification {
    fn default() -> Self {
        Self::new()
    }
}

impl ProofOfVerification {
    /// Create a new PoV engine
    #[must_use]
    pub fn new() -> Self {
        Self {
            verification_cache: HashMap::new(),
            cache_ttl_ms: 60_000, // 1 minute
        }
    }

    /// Verify a solution against its job specification
    ///
    /// This is the core "mining" operation in HardClaw.
    pub fn verify_solution(
        &mut self,
        job: &JobPacket,
        solution: &SolutionCandidate,
        verifier_keypair: &Keypair,
    ) -> Result<VerificationResult, ConsensusError> {
        // Check job-solution matching
        if solution.job_id != job.id {
            return Err(ConsensusError::SolutionMismatch);
        }

        // Check if we have a cached result
        if let Some(cached) = self.get_cached_result(&solution.id) {
            return Ok(cached.clone());
        }

        let start = Instant::now();

        // Perform verification based on job type
        let (passed, error) = match &job.verification {
            VerificationSpec::HashMatch { expected_hash } => {
                self.verify_hash_match(&solution.output, expected_hash)
            }

            VerificationSpec::WasmVerifier { module_hash, entry_point } => {
                self.verify_wasm(&job.input, &solution.output, module_hash, entry_point)
            }

            VerificationSpec::SchellingPoint { .. } => {
                // Schelling point verification is handled separately
                return Err(ConsensusError::VerificationFailed {
                    reason: "Subjective tasks require Schelling consensus".to_string(),
                });
            }
        };

        let verification_time_ms = start.elapsed().as_millis() as u64;

        // Create and sign the verification result
        let mut result = VerificationResult::new(
            solution.id,
            job.id,
            *verifier_keypair.public_key(),
            passed,
            error,
            verification_time_ms,
        );

        result.signature = verifier_keypair.sign(&result.signing_bytes());

        // Cache the result
        self.cache_result(solution.id, result.clone());

        Ok(result)
    }

    /// Verify hash match (for deterministic tasks)
    fn verify_hash_match(&self, output: &[u8], expected_hash: &Hash) -> (bool, Option<String>) {
        let actual_hash = hash_data(output);

        if actual_hash == *expected_hash {
            (true, None)
        } else {
            (false, Some(format!(
                "Hash mismatch: expected {}, got {}",
                expected_hash.to_hex(),
                actual_hash.to_hex()
            )))
        }
    }

    /// Verify using WASM module
    ///
    /// NOTE: Full WASM verification would require a WASM runtime.
    /// This is a placeholder that validates the module hash.
    fn verify_wasm(
        &self,
        _input: &[u8],
        _output: &[u8],
        module_hash: &Hash,
        _entry_point: &str,
    ) -> (bool, Option<String>) {
        // In a full implementation, this would:
        // 1. Load the WASM module from storage
        // 2. Verify its hash matches module_hash
        // 3. Execute the entry_point function with (input, output)
        // 4. Return the boolean result

        // For now, we just validate that a module hash was provided
        if *module_hash == Hash::ZERO {
            return (false, Some("Invalid WASM module hash".to_string()));
        }

        // Placeholder: would execute WASM here
        (true, None)
    }

    /// Validate a complete block
    pub fn validate_block(
        &self,
        block: &Block,
        parent: Option<&Block>,
        active_verifiers: usize,
    ) -> Result<(), ConsensusError> {
        // Check parent reference
        if let Some(parent_block) = parent {
            if block.header.parent_hash != parent_block.hash {
                return Err(ConsensusError::InvalidParent);
            }

            if block.header.height != parent_block.header.height + 1 {
                return Err(ConsensusError::HeightMismatch {
                    expected: parent_block.header.height + 1,
                    got: block.header.height,
                });
            }
        } else if block.header.height != 0 {
            // Non-genesis block must have a parent
            return Err(ConsensusError::InvalidParent);
        }

        // Check block integrity
        block.verify_integrity().map_err(|e| ConsensusError::VerificationFailed {
            reason: e.to_string(),
        })?;

        // Check consensus threshold (66%)
        if !block.has_consensus(active_verifiers) {
            let percentage = block.consensus_percentage(active_verifiers) * 100.0;
            return Err(ConsensusError::InsufficientConsensus { percentage });
        }

        // Verify all attestation signatures
        for attestation in &block.attestations {
            attestation.verify_signature().map_err(|_| {
                ConsensusError::VerificationFailed {
                    reason: "Invalid attestation signature".to_string(),
                }
            })?;
        }

        Ok(())
    }

    /// Create an attestation for a block
    pub fn create_attestation(
        &self,
        block: &Block,
        verified_solutions: Vec<Hash>,
        verifier_keypair: &Keypair,
    ) -> VerifierAttestation {
        let mut attestation = VerifierAttestation::new(
            *verifier_keypair.public_key(),
            block.hash,
            verified_solutions,
        );

        attestation.signature = verifier_keypair.sign(&attestation.signing_bytes());
        attestation
    }

    /// Get cached verification result
    fn get_cached_result(&self, solution_id: &Hash) -> Option<&VerificationResult> {
        self.verification_cache.get(solution_id).filter(|result| {
            let age = now_millis() - result.verified_at;
            age < self.cache_ttl_ms
        })
    }

    /// Cache a verification result
    fn cache_result(&mut self, solution_id: Hash, result: VerificationResult) {
        self.verification_cache.insert(solution_id, result);
    }

    /// Clear expired cache entries
    pub fn cleanup_cache(&mut self) {
        let now = now_millis();
        self.verification_cache.retain(|_, result| {
            now - result.verified_at < self.cache_ttl_ms
        });
    }
}

impl SolutionVerifier for ProofOfVerification {
    fn verify(
        &self,
        _job: &JobPacket,
        _solution: &SolutionCandidate,
    ) -> Result<VerificationResult, ConsensusError> {
        // This trait method can't mutate self, so we create a temporary keypair
        // In practice, the verify_solution method with keypair should be used
        Err(ConsensusError::VerificationFailed {
            reason: "Use verify_solution with keypair instead".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{JobType, HclawAmount};

    fn create_test_job_and_solution() -> (JobPacket, SolutionCandidate, Keypair, Keypair) {
        let requester_kp = Keypair::generate();
        let solver_kp = Keypair::generate();

        let output = b"correct output";
        let expected_hash = hash_data(output);

        let mut job = JobPacket::new(
            JobType::Deterministic,
            *requester_kp.public_key(),
            b"input data".to_vec(),
            "Test job".to_string(),
            HclawAmount::from_hclaw(100),
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

        (job, solution, requester_kp, solver_kp)
    }

    #[test]
    fn test_verify_hash_match_success() {
        let (job, solution, _, _) = create_test_job_and_solution();
        let verifier_kp = Keypair::generate();

        let mut pov = ProofOfVerification::new();
        let result = pov.verify_solution(&job, &solution, &verifier_kp).unwrap();

        assert!(result.passed);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_verify_hash_match_failure() {
        let (job, _, _, solver_kp) = create_test_job_and_solution();
        let verifier_kp = Keypair::generate();

        // Create solution with wrong output
        let mut bad_solution = SolutionCandidate::new(
            job.id,
            *solver_kp.public_key(),
            b"wrong output".to_vec(),
        );
        bad_solution.signature = solver_kp.sign(&bad_solution.signing_bytes());

        let mut pov = ProofOfVerification::new();
        let result = pov.verify_solution(&job, &bad_solution, &verifier_kp).unwrap();

        assert!(!result.passed);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_job_solution_mismatch() {
        let (job, _, _, solver_kp) = create_test_job_and_solution();
        let verifier_kp = Keypair::generate();

        // Create solution for different job
        let mut wrong_job_solution = SolutionCandidate::new(
            Hash::ZERO, // Wrong job ID
            *solver_kp.public_key(),
            b"output".to_vec(),
        );
        wrong_job_solution.signature = solver_kp.sign(&wrong_job_solution.signing_bytes());

        let mut pov = ProofOfVerification::new();
        let result = pov.verify_solution(&job, &wrong_job_solution, &verifier_kp);

        assert!(matches!(result, Err(ConsensusError::SolutionMismatch)));
    }

    #[test]
    fn test_verification_caching() {
        let (job, solution, _, _) = create_test_job_and_solution();
        let verifier_kp = Keypair::generate();

        let mut pov = ProofOfVerification::new();

        // First verification
        let result1 = pov.verify_solution(&job, &solution, &verifier_kp).unwrap();

        // Second verification should use cache (same result)
        let result2 = pov.verify_solution(&job, &solution, &verifier_kp).unwrap();

        assert_eq!(result1.passed, result2.passed);
        assert_eq!(result1.solution_id, result2.solution_id);
    }

    #[test]
    fn test_block_validation() {
        let verifier_kp = Keypair::generate();
        let pov = ProofOfVerification::new();

        // Create genesis block
        let genesis = Block::genesis(*verifier_kp.public_key());

        // Genesis should validate with 0 verifiers (special case)
        // Actually, with 0 verifiers, consensus check would fail, so we need at least 1
        // Let's add an attestation
        let mut genesis_with_attestation = genesis.clone();
        let attestation = VerifierAttestation::new(
            *verifier_kp.public_key(),
            genesis.hash,
            Vec::new(),
        );
        let mut attestation = attestation;
        attestation.signature = verifier_kp.sign(&attestation.signing_bytes());
        genesis_with_attestation.add_attestation(attestation);

        assert!(pov.validate_block(&genesis_with_attestation, None, 1).is_ok());
    }
}
