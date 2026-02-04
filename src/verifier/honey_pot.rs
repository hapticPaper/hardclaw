//! Honey Pot defense mechanism against lazy miners.
//!
//! ## The Lazy Miner Attack
//!
//! Attack: Miners approve every solution without actually verifying to save CPU.
//! This undermines the entire verification-based consensus.
//!
//! ## Defense: Randomized Honey Pots
//!
//! The protocol injects valid-looking but *invalid* solutions into the mempool.
//! These honey pots:
//! - Look legitimate (proper signatures, valid job references)
//! - Fail verification (wrong output hash, invalid proof)
//!
//! If a miner signs (approves) a honey pot, their entire stake is slashed.
//! This makes lazy mining economically irrational.

use std::collections::HashSet;
use std::sync::RwLock;

use rand::Rng;

use crate::crypto::{Hash, PublicKey};
use crate::types::{JobPacket, SolutionCandidate};

/// Generates honey pot solutions to detect lazy miners
pub struct HoneyPotGenerator {
    /// Injection rate (0.0 - 1.0)
    injection_rate: f64,
    /// Generated honey pot IDs (for tracking)
    generated_ids: RwLock<HashSet<Hash>>,
    /// Random number generator seed (for reproducibility in tests)
    seed: u64,
}

impl HoneyPotGenerator {
    /// Create a new honey pot generator
    #[must_use]
    pub fn new(injection_rate: f64) -> Self {
        let injection_rate = injection_rate.clamp(0.0, 1.0);

        Self {
            injection_rate,
            generated_ids: RwLock::new(HashSet::new()),
            seed: rand::thread_rng().gen(),
        }
    }

    /// Create with a specific seed (for testing)
    #[must_use]
    pub fn with_seed(injection_rate: f64, seed: u64) -> Self {
        Self {
            injection_rate: injection_rate.clamp(0.0, 1.0),
            generated_ids: RwLock::new(HashSet::new()),
            seed,
        }
    }

    /// Decide whether to inject a honey pot for this job
    #[must_use]
    pub fn should_inject(&self) -> bool {
        rand::thread_rng().gen::<f64>() < self.injection_rate
    }

    /// Generate a honey pot solution for a job
    ///
    /// The honey pot will:
    /// - Reference the correct job ID
    /// - Have a valid-looking structure
    /// - Contain deliberately wrong output that will fail verification
    #[must_use]
    pub fn generate(&self, job: &JobPacket, fake_solver: &PublicKey) -> SolutionCandidate {
        // Generate fake output that looks plausible but is wrong
        let fake_output = self.generate_fake_output(job);

        // Create the honey pot solution
        let solution = SolutionCandidate::create_honey_pot(
            job.id,
            *fake_solver,
            fake_output,
        );

        // Track this honey pot
        if let Ok(mut ids) = self.generated_ids.write() {
            ids.insert(solution.id);
        }

        solution
    }

    /// Generate fake output that looks legitimate but is wrong
    fn generate_fake_output(&self, job: &JobPacket) -> Vec<u8> {
        // The fake output should be similar in structure to real output
        // but cryptographically different.

        // Strategy: Take the job input and XOR with random bytes
        let mut fake = job.input.clone();
        if fake.is_empty() {
            fake = vec![0u8; 32];
        }

        // Modify to ensure it's different
        let random_bytes: Vec<u8> = (0..fake.len())
            .map(|i| {
                let mut rng = rand::thread_rng();
                rng.gen::<u8>().wrapping_add(i as u8)
            })
            .collect();

        for (i, byte) in fake.iter_mut().enumerate() {
            *byte ^= random_bytes[i % random_bytes.len()];
        }

        // Ensure it's definitely not the correct hash
        // by appending a marker
        fake.extend_from_slice(b"__HONEYPOT__");

        fake
    }

    /// Check if a solution ID is a known honey pot
    #[must_use]
    pub fn is_honey_pot(&self, solution_id: &Hash) -> bool {
        self.generated_ids
            .read()
            .map(|ids| ids.contains(solution_id))
            .unwrap_or(false)
    }

    /// Get count of generated honey pots
    #[must_use]
    pub fn honey_pot_count(&self) -> usize {
        self.generated_ids
            .read()
            .map(|ids| ids.len())
            .unwrap_or(0)
    }

    /// Clear old honey pot records (called periodically)
    pub fn cleanup(&self, keep_ids: &HashSet<Hash>) {
        if let Ok(mut ids) = self.generated_ids.write() {
            ids.retain(|id| keep_ids.contains(id));
        }
    }
}

/// Detects honey pot solutions and tracks offending miners
pub struct HoneyPotDetector {
    /// Known honey pot solution IDs
    known_honey_pots: RwLock<HashSet<Hash>>,
    /// Miners who approved honey pots (to be slashed)
    offenders: RwLock<HashSet<PublicKey>>,
}

impl Default for HoneyPotDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl HoneyPotDetector {
    /// Create a new detector
    #[must_use]
    pub fn new() -> Self {
        Self {
            known_honey_pots: RwLock::new(HashSet::new()),
            offenders: RwLock::new(HashSet::new()),
        }
    }

    /// Register a honey pot solution ID
    pub fn register(&self, solution_id: &Hash) {
        if let Ok(mut pots) = self.known_honey_pots.write() {
            pots.insert(*solution_id);
        }
    }

    /// Check if a solution is a known honey pot
    #[must_use]
    pub fn is_honey_pot(&self, solution_id: &Hash) -> bool {
        self.known_honey_pots
            .read()
            .map(|pots| pots.contains(solution_id))
            .unwrap_or(false)
    }

    /// Record that a miner approved a honey pot
    pub fn record_offender(&self, miner: &PublicKey, solution_id: &Hash) {
        if self.is_honey_pot(solution_id) {
            if let Ok(mut offenders) = self.offenders.write() {
                offenders.insert(*miner);
            }
        }
    }

    /// Check if a miner is a known offender
    #[must_use]
    pub fn is_offender(&self, miner: &PublicKey) -> bool {
        self.offenders
            .read()
            .map(|offenders| offenders.contains(miner))
            .unwrap_or(false)
    }

    /// Get all offenders for slashing
    #[must_use]
    pub fn get_offenders(&self) -> Vec<PublicKey> {
        self.offenders
            .read()
            .map(|offenders| offenders.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Clear an offender after slashing
    pub fn clear_offender(&self, miner: &PublicKey) {
        if let Ok(mut offenders) = self.offenders.write() {
            offenders.remove(miner);
        }
    }

    /// Get statistics
    #[must_use]
    pub fn stats(&self) -> HoneyPotStats {
        HoneyPotStats {
            total_honey_pots: self.known_honey_pots.read().map(|p| p.len()).unwrap_or(0),
            total_offenders: self.offenders.read().map(|o| o.len()).unwrap_or(0),
        }
    }
}

/// Honey pot statistics
#[derive(Clone, Debug, Default)]
pub struct HoneyPotStats {
    /// Total honey pots in circulation
    pub total_honey_pots: usize,
    /// Total miners who approved honey pots
    pub total_offenders: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{hash_data, Keypair};
    use crate::types::{JobType, HclawAmount, VerificationSpec};

    fn create_test_job() -> JobPacket {
        let kp = Keypair::generate();
        JobPacket::new(
            JobType::Deterministic,
            *kp.public_key(),
            b"test input".to_vec(),
            "Test job".to_string(),
            HclawAmount::from_hclaw(10),
            HclawAmount::from_hclaw(1),
            VerificationSpec::HashMatch {
                expected_hash: hash_data(b"correct output"),
            },
            3600,
        )
    }

    #[test]
    fn test_honey_pot_generation() {
        let generator = HoneyPotGenerator::new(1.0); // Always inject
        let job = create_test_job();
        let fake_solver = Keypair::generate();

        let honey_pot = generator.generate(&job, fake_solver.public_key());

        assert!(honey_pot.is_honey_pot);
        assert_eq!(honey_pot.job_id, job.id);
        assert!(generator.is_honey_pot(&honey_pot.id));
    }

    #[test]
    fn test_honey_pot_output_is_wrong() {
        let generator = HoneyPotGenerator::new(1.0);
        let job = create_test_job();
        let fake_solver = Keypair::generate();

        let honey_pot = generator.generate(&job, fake_solver.public_key());

        // The honey pot output should NOT match the expected hash
        let expected_hash = match &job.verification {
            VerificationSpec::HashMatch { expected_hash } => expected_hash,
            _ => panic!("Expected HashMatch"),
        };

        assert_ne!(honey_pot.output_hash, *expected_hash);
    }

    #[test]
    fn test_injection_rate() {
        let generator = HoneyPotGenerator::with_seed(0.0, 12345);

        // With 0% rate, should never inject
        let mut injected = 0;
        for _ in 0..1000 {
            if generator.should_inject() {
                injected += 1;
            }
        }
        assert_eq!(injected, 0);
    }

    #[test]
    fn test_detector() {
        let detector = HoneyPotDetector::new();
        let solution_id = hash_data(b"honey pot");
        let miner = Keypair::generate();

        // Register honey pot
        detector.register(&solution_id);
        assert!(detector.is_honey_pot(&solution_id));

        // Record offender
        detector.record_offender(miner.public_key(), &solution_id);
        assert!(detector.is_offender(miner.public_key()));

        // Clear after slashing
        detector.clear_offender(miner.public_key());
        assert!(!detector.is_offender(miner.public_key()));
    }
}
