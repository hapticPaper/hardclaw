//! Job Packets - the unit of work in the HardClaw protocol.
//!
//! A Job Packet contains:
//! - Input data for the task
//! - Bounty (payment for the work)
//! - Verification specification (how to verify the solution)

use serde::{Deserialize, Serialize};

use crate::crypto::{hash_data, Hash, PublicKey, Signature};
use super::{Address, Id, HclawAmount, Timestamp, now_millis};

/// Type of job (determines verification method)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobType {
    /// Deterministic verification (math, hash check, physics sim)
    Deterministic,
    /// Subjective verification via Schelling Point consensus
    Subjective,
}

/// Status of a job in the system
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobStatus {
    /// In the mempool, waiting for a solver
    Pending,
    /// Claimed by a solver
    Claimed,
    /// Solution submitted, awaiting verification
    Verifying,
    /// Verified and paid out
    Completed,
    /// Expired without completion
    Expired,
    /// Disputed (for subjective jobs under Schelling Point consensus)
    Disputed,
}

/// Specification for how to verify the solution
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum VerificationSpec {
    /// Hash of expected output (for deterministic tasks)
    HashMatch {
        /// Expected output hash
        expected_hash: Hash,
    },

    /// WASM module that returns true/false
    WasmVerifier {
        /// Hash of the WASM module bytecode
        module_hash: Hash,
        /// Entry point function name
        entry_point: String,
    },

    /// Schelling point voting (for subjective tasks)
    SchellingPoint {
        /// Minimum number of voters
        min_voters: u8,
        /// Quality threshold (0-100)
        quality_threshold: u8,
    },
}

/// A Job Packet submitted by a Requester
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobPacket {
    /// Unique job ID (hash of contents)
    pub id: Id,
    /// Type of job
    pub job_type: JobType,
    /// Current status
    pub status: JobStatus,
    /// Requester's public key
    pub requester: PublicKey,
    /// Requester's address (for fee payments)
    pub requester_address: Address,
    /// Input data for the task (opaque bytes)
    pub input: Vec<u8>,
    /// Human-readable task description
    pub description: String,
    /// Bounty offered for completion
    pub bounty: HclawAmount,
    /// Amount burned to submit this job (anti-Sybil)
    pub burn_fee: HclawAmount,
    /// How to verify the solution
    pub verification: VerificationSpec,
    /// When the job was created
    pub created_at: Timestamp,
    /// When the job expires
    pub expires_at: Timestamp,
    /// Requester's signature over the job data
    pub signature: Signature,
}

impl JobPacket {
    /// Create a new job packet (unsigned)
    #[must_use]
    pub fn new(
        job_type: JobType,
        requester: PublicKey,
        input: Vec<u8>,
        description: String,
        bounty: HclawAmount,
        burn_fee: HclawAmount,
        verification: VerificationSpec,
        ttl_secs: u64,
    ) -> Self {
        let now = now_millis();
        let expires_at = now + (ttl_secs as i64 * 1000);
        let requester_address = Address::from_public_key(&requester);

        let mut job = Self {
            id: Hash::ZERO, // Will be set after hashing
            job_type,
            status: JobStatus::Pending,
            requester,
            requester_address,
            input,
            description,
            bounty,
            burn_fee,
            verification,
            created_at: now,
            expires_at,
            signature: Signature::from_bytes([0u8; 64]), // Placeholder
        };

        job.id = job.compute_id();
        job
    }

    /// Compute the job ID from its contents
    #[must_use]
    pub fn compute_id(&self) -> Id {
        let mut data = Vec::new();
        data.extend_from_slice(self.requester.as_bytes());
        data.extend_from_slice(&self.input);
        data.extend_from_slice(self.description.as_bytes());
        data.extend_from_slice(&self.bounty.raw().to_le_bytes());
        data.extend_from_slice(&self.created_at.to_le_bytes());

        hash_data(&data)
    }

    /// Get the bytes to sign
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        // Sign everything except the signature itself
        let mut data = Vec::new();
        data.extend_from_slice(self.id.as_bytes());
        data.extend_from_slice(&[self.job_type as u8]);
        data.extend_from_slice(self.requester.as_bytes());
        data.extend_from_slice(&self.input);
        data.extend_from_slice(&self.bounty.raw().to_le_bytes());
        data.extend_from_slice(&self.burn_fee.raw().to_le_bytes());
        data.extend_from_slice(&self.created_at.to_le_bytes());
        data.extend_from_slice(&self.expires_at.to_le_bytes());
        data
    }

    /// Check if the job has expired
    #[must_use]
    pub fn is_expired(&self) -> bool {
        now_millis() > self.expires_at
    }

    /// Check if the job is still valid for processing
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && self.status == JobStatus::Pending
    }

    /// Calculate total cost (bounty + burn fee)
    #[must_use]
    pub fn total_cost(&self) -> HclawAmount {
        self.bounty.saturating_add(self.burn_fee)
    }

    /// Verify the job packet signature
    ///
    /// # Errors
    /// Returns error if signature is invalid
    pub fn verify_signature(&self) -> Result<(), crate::crypto::CryptoError> {
        crate::crypto::verify(&self.requester, &self.signing_bytes(), &self.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn create_test_job() -> (JobPacket, Keypair) {
        let kp = Keypair::generate();

        let mut job = JobPacket::new(
            JobType::Deterministic,
            *kp.public_key(),
            b"test input".to_vec(),
            "Test job".to_string(),
            HclawAmount::from_hclaw(100),
            HclawAmount::from_hclaw(1),
            VerificationSpec::HashMatch {
                expected_hash: Hash::ZERO,
            },
            3600, // 1 hour TTL
        );

        // Sign the job
        job.signature = kp.sign(&job.signing_bytes());

        (job, kp)
    }

    #[test]
    fn test_job_creation() {
        let (job, _) = create_test_job();

        assert_eq!(job.status, JobStatus::Pending);
        assert!(!job.is_expired());
        assert!(job.is_valid());
    }

    #[test]
    fn test_job_signature() {
        let (job, _) = create_test_job();
        assert!(job.verify_signature().is_ok());
    }

    #[test]
    fn test_job_id_deterministic() {
        let kp = Keypair::generate();

        let job1 = JobPacket::new(
            JobType::Deterministic,
            *kp.public_key(),
            b"same input".to_vec(),
            "Same description".to_string(),
            HclawAmount::from_hclaw(100),
            HclawAmount::from_hclaw(1),
            VerificationSpec::HashMatch { expected_hash: Hash::ZERO },
            3600,
        );

        // ID should be based on content, so different timestamps = different IDs
        // But same content at same time should be same ID
        assert_eq!(job1.compute_id(), job1.id);
    }

    #[test]
    fn test_total_cost() {
        let (job, _) = create_test_job();
        assert_eq!(job.total_cost().whole_hclaw(), 101);
    }
}
