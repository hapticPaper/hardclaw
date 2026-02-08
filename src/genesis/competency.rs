//! One-time competency challenges for new verifiers.
//!
//! Before a verifier can be activated and receive their airdrop,
//! they must prove they can actually verify solutions correctly.
//! The challenge presents a known-good and known-bad solution;
//! the verifier must accept the good one and reject the bad one.
//!
//! Pre-approved addresses (bootstrap nodes, founder machines) skip this check.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::crypto::{hash_data, Hash};
use crate::types::{Address, Timestamp};

/// A competency challenge for a new verifier
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompetencyChallenge {
    /// Unique challenge ID
    pub id: Hash,
    /// The verifier being tested
    pub verifier: Address,
    /// Hash of the valid test solution (verifier should accept this)
    pub valid_solution_hash: Hash,
    /// Hash of the invalid test solution (verifier should reject this)
    pub invalid_solution_hash: Hash,
    /// When the challenge was issued
    pub issued_at: Timestamp,
    /// Challenge expiry
    pub expires_at: Timestamp,
    /// Challenge status
    pub status: ChallengeStatus,
}

/// Status of a competency challenge
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeStatus {
    /// Awaiting verifier's responses
    Pending,
    /// Verifier passed
    Passed,
    /// Verifier failed
    Failed {
        /// What went wrong
        reason: String,
    },
    /// Challenge expired without response
    Expired,
}

/// Challenge timeout (5 minutes)
const CHALLENGE_TIMEOUT_MS: i64 = 5 * 60 * 1000;

/// Cooldown between retries (1 minute)
const RETRY_COOLDOWN_MS: i64 = 60 * 1000;

/// Maximum retry attempts
const MAX_ATTEMPTS: u32 = 5;

/// Manages competency challenges for verifier onboarding
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompetencyManager {
    /// Active challenges by verifier address
    challenges: HashMap<Address, CompetencyChallenge>,
    /// Addresses that have passed competency
    verified: HashSet<Address>,
    /// Pre-approved addresses (skip challenge entirely)
    pre_approved: HashSet<Address>,
    /// Failed attempt counts per address
    fail_counts: HashMap<Address, u32>,
    /// Last attempt timestamp per address (for cooldown)
    last_attempt: HashMap<Address, Timestamp>,
}

impl CompetencyManager {
    /// Create a new manager with pre-approved addresses
    #[must_use]
    pub fn new(pre_approved: &[Address]) -> Self {
        let mut verified = HashSet::new();
        let pre_approved_set: HashSet<Address> = pre_approved.iter().copied().collect();

        // Pre-approved addresses are automatically verified
        for addr in pre_approved {
            verified.insert(*addr);
        }

        Self {
            challenges: HashMap::new(),
            verified,
            pre_approved: pre_approved_set,
            fail_counts: HashMap::new(),
            last_attempt: HashMap::new(),
        }
    }

    /// Check if an address is verified (passed competency or pre-approved)
    #[must_use]
    pub fn is_verified(&self, address: &Address) -> bool {
        self.verified.contains(address)
    }

    /// Check if an address is pre-approved
    #[must_use]
    pub fn is_pre_approved(&self, address: &Address) -> bool {
        self.pre_approved.contains(address)
    }

    /// Generate a challenge for a verifier.
    /// Returns `None` if already verified, or error if rate-limited.
    pub fn generate_challenge(
        &mut self,
        verifier: Address,
        now: Timestamp,
    ) -> Result<CompetencyChallenge, CompetencyError> {
        if self.verified.contains(&verifier) {
            return Err(CompetencyError::AlreadyVerified);
        }

        if let Some(&count) = self.fail_counts.get(&verifier) {
            if count >= MAX_ATTEMPTS {
                return Err(CompetencyError::TooManyAttempts);
            }
        }

        // Check cooldown
        if let Some(&last) = self.last_attempt.get(&verifier) {
            if now - last < RETRY_COOLDOWN_MS {
                return Err(CompetencyError::CooldownActive {
                    retry_after: last + RETRY_COOLDOWN_MS,
                });
            }
        }

        // Generate deterministic challenge seeded from verifier address + timestamp
        let seed = hash_data(
            &[verifier.as_bytes().as_slice(), &now.to_le_bytes()].concat(),
        );

        // Create deterministic valid/invalid solution hashes from the seed
        let valid_hash = hash_data(&[seed.as_bytes().as_slice(), b"valid"].concat());
        let invalid_hash = hash_data(&[seed.as_bytes().as_slice(), b"invalid"].concat());

        let challenge = CompetencyChallenge {
            id: seed,
            verifier,
            valid_solution_hash: valid_hash,
            invalid_solution_hash: invalid_hash,
            issued_at: now,
            expires_at: now + CHALLENGE_TIMEOUT_MS,
            status: ChallengeStatus::Pending,
        };

        self.challenges.insert(verifier, challenge.clone());
        self.last_attempt.insert(verifier, now);

        Ok(challenge)
    }

    /// Submit challenge results.
    ///
    /// `accepted_valid`: did the verifier accept the valid solution?
    /// `rejected_invalid`: did the verifier reject the invalid solution?
    pub fn submit_result(
        &mut self,
        verifier: &Address,
        accepted_valid: bool,
        rejected_invalid: bool,
        now: Timestamp,
    ) -> Result<ChallengeStatus, CompetencyError> {
        let challenge = self
            .challenges
            .get_mut(verifier)
            .ok_or(CompetencyError::NoPendingChallenge)?;

        if challenge.status != ChallengeStatus::Pending {
            return Err(CompetencyError::NoPendingChallenge);
        }

        if now > challenge.expires_at {
            challenge.status = ChallengeStatus::Expired;
            return Err(CompetencyError::ChallengeExpired);
        }

        if accepted_valid && rejected_invalid {
            challenge.status = ChallengeStatus::Passed;
            self.verified.insert(*verifier);
            Ok(ChallengeStatus::Passed)
        } else {
            let reason = match (accepted_valid, rejected_invalid) {
                (false, false) => {
                    "failed to accept valid solution AND failed to reject invalid solution"
                }
                (false, true) => "failed to accept the valid solution",
                (true, false) => "failed to reject the invalid solution",
                _ => unreachable!(),
            };

            challenge.status = ChallengeStatus::Failed {
                reason: reason.to_string(),
            };
            *self.fail_counts.entry(*verifier).or_insert(0) += 1;

            Ok(ChallengeStatus::Failed {
                reason: reason.to_string(),
            })
        }
    }

    /// Get a pending challenge for a verifier
    #[must_use]
    pub fn get_challenge(&self, verifier: &Address) -> Option<&CompetencyChallenge> {
        self.challenges.get(verifier)
    }

    /// Number of verified verifiers
    #[must_use]
    pub fn verified_count(&self) -> usize {
        self.verified.len()
    }
}

/// Competency challenge errors
#[derive(Debug, thiserror::Error)]
pub enum CompetencyError {
    /// Verifier already passed
    #[error("verifier already passed competency check")]
    AlreadyVerified,
    /// Too many failed attempts
    #[error("too many failed attempts (max {MAX_ATTEMPTS})")]
    TooManyAttempts,
    /// No pending challenge
    #[error("no pending challenge for this verifier")]
    NoPendingChallenge,
    /// Challenge expired
    #[error("challenge expired")]
    ChallengeExpired,
    /// Cooldown active
    #[error("retry cooldown active, try again after {retry_after}")]
    CooldownActive {
        /// When the cooldown expires
        retry_after: Timestamp,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_addr() -> Address {
        Address::from_public_key(Keypair::generate().public_key())
    }

    #[test]
    fn test_pre_approved_auto_verified() {
        let addr = test_addr();
        let manager = CompetencyManager::new(&[addr]);

        assert!(manager.is_verified(&addr));
        assert!(manager.is_pre_approved(&addr));
    }

    #[test]
    fn test_challenge_flow_pass() {
        let pre = test_addr();
        let mut manager = CompetencyManager::new(&[pre]);

        let verifier = test_addr();
        assert!(!manager.is_verified(&verifier));

        // Generate challenge
        let challenge = manager.generate_challenge(verifier, 1000).unwrap();
        assert_eq!(challenge.status, ChallengeStatus::Pending);

        // Submit correct results
        let status = manager.submit_result(&verifier, true, true, 2000).unwrap();
        assert_eq!(status, ChallengeStatus::Passed);
        assert!(manager.is_verified(&verifier));
    }

    #[test]
    fn test_challenge_flow_fail() {
        let mut manager = CompetencyManager::new(&[]);
        let verifier = test_addr();

        manager.generate_challenge(verifier, 1000).unwrap();

        // Failed: accepted invalid solution
        let status = manager.submit_result(&verifier, true, false, 2000).unwrap();
        assert!(matches!(status, ChallengeStatus::Failed { .. }));
        assert!(!manager.is_verified(&verifier));
    }

    #[test]
    fn test_max_attempts() {
        let mut manager = CompetencyManager::new(&[]);
        let verifier = test_addr();

        for i in 0..MAX_ATTEMPTS {
            let time = (i as i64) * (RETRY_COOLDOWN_MS + 1000);
            manager.generate_challenge(verifier, time).unwrap();
            manager
                .submit_result(&verifier, false, false, time + 1000)
                .unwrap();
        }

        // Next attempt should fail
        let time = (MAX_ATTEMPTS as i64) * (RETRY_COOLDOWN_MS + 1000);
        let result = manager.generate_challenge(verifier, time);
        assert!(matches!(result, Err(CompetencyError::TooManyAttempts)));
    }

    #[test]
    fn test_cooldown() {
        let mut manager = CompetencyManager::new(&[]);
        let verifier = test_addr();

        manager.generate_challenge(verifier, 1000).unwrap();
        manager
            .submit_result(&verifier, false, false, 2000)
            .unwrap();

        // Try again too soon
        let result = manager.generate_challenge(verifier, 3000);
        assert!(matches!(result, Err(CompetencyError::CooldownActive { .. })));

        // After cooldown
        let result = manager.generate_challenge(verifier, 1000 + RETRY_COOLDOWN_MS + 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_already_verified_cant_rechallenge() {
        let mut manager = CompetencyManager::new(&[]);
        let verifier = test_addr();

        manager.generate_challenge(verifier, 1000).unwrap();
        manager.submit_result(&verifier, true, true, 2000).unwrap();

        let result = manager.generate_challenge(verifier, 3000);
        assert!(matches!(result, Err(CompetencyError::AlreadyVerified)));
    }
}
