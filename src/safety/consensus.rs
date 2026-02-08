//! Consensus engine for safety reviews.

use crate::types::review::{ConsensusDecision, SafetyConsensus, SafetyReviewVote, SafetyVerdict};

/// Safety consensus engine
pub struct SafetyConsensusEngine {
    /// Minimum number of reviewers required
    pub min_reviewers: usize,
    /// Timeout for commit phase (ms)
    pub commit_timeout_ms: u64,
    /// Timeout for reveal phase (ms)
    pub reveal_timeout_ms: u64,
}

impl SafetyConsensusEngine {
    /// Create a new consensus engine
    pub fn new() -> Self {
        Self {
            min_reviewers: 5,           // At least 5 reviewers required
            commit_timeout_ms: 300_000, // 5 minutes
            reveal_timeout_ms: 300_000, // 5 minutes
        }
    }

    /// Calculate consensus from votes
    pub fn calculate_consensus(
        &self,
        code_hash: crate::crypto::Hash,
        votes: Vec<SafetyReviewVote>,
    ) -> SafetyConsensus {
        SafetyConsensus::from_votes(code_hash, votes)
    }

    /// Check if consensus is valid
    pub fn is_valid_consensus(&self, consensus: &SafetyConsensus) -> bool {
        // Must have minimum number of votes
        if consensus.total_reviewers < self.min_reviewers {
            return false;
        }

        // Decision must not be "insufficient votes"
        consensus.decision != ConsensusDecision::InsufficientVotes
    }

    /// Calculate Schelling point incentives
    ///
    /// Validators are rewarded for being in the majority, creating a
    /// natural incentive to provide accurate assessments without
    /// needing to coordinate.
    pub fn calculate_schelling_rewards(
        &self,
        consensus: &SafetyConsensus,
    ) -> Vec<(crate::crypto::PublicKey, f64)> {
        let mut rewards = Vec::new();

        // Determine what the "correct" answer is based on consensus
        let majority_verdict = match consensus.decision {
            ConsensusDecision::ApprovedStrong | ConsensusDecision::ApprovedWeak => {
                SafetyVerdict::Safe
            }
            ConsensusDecision::RejectedStrong | ConsensusDecision::RejectedWeak => {
                SafetyVerdict::Unsafe
            }
            _ => return rewards, // No rewards for unclear cases
        };

        for vote in &consensus.votes {
            // Base reward for voting
            let mut reward = 1.0;

            // Bonus for being in majority (Schelling point)
            if vote.verdict == majority_verdict {
                reward *= 1.5;
            } else {
                // Penalty for being in minority
                reward *= 0.5;
            }

            // Confidence-weighted rewards
            reward *= vote.confidence;

            rewards.push((vote.reviewer.clone(), reward));
        }

        rewards
    }

    /// Detect potential collusion or gaming
    pub fn detect_anomalies(&self, consensus: &SafetyConsensus) -> Vec<AnomalyAlert> {
        let mut alerts = Vec::new();

        // Check for unanimous votes (potential collusion)
        if consensus.safe_votes == consensus.total_reviewers
            || consensus.unsafe_votes == consensus.total_reviewers
        {
            alerts.push(AnomalyAlert::UnanimousVote {
                code_hash: consensus.code_hash,
                verdict: if consensus.safe_votes == consensus.total_reviewers {
                    SafetyVerdict::Safe
                } else {
                    SafetyVerdict::Unsafe
                },
            });
        }

        // Check for very low confidence scores (uncertain reviewers)
        let low_confidence_count = consensus
            .votes
            .iter()
            .filter(|v| v.confidence < 0.3)
            .count();

        if low_confidence_count as f64 / consensus.total_reviewers as f64 > 0.5 {
            alerts.push(AnomalyAlert::LowConfidence {
                code_hash: consensus.code_hash,
                low_confidence_fraction: low_confidence_count as f64
                    / consensus.total_reviewers as f64,
            });
        }

        // Check for 51% attack patterns (all votes from low-reputation reviewers)
        // This would require reputation data from the manager

        alerts
    }
}

impl Default for SafetyConsensusEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Anomaly detection alerts
#[derive(Debug, Clone)]
pub enum AnomalyAlert {
    /// All reviewers voted the same way (potential collusion)
    UnanimousVote {
        /// Hash of the reviewed code
        code_hash: crate::crypto::Hash,
        /// Consensus verdict
        verdict: SafetyVerdict,
    },
    /// Too many reviewers with low confidence
    LowConfidence {
        /// Hash of the reviewed code
        code_hash: crate::crypto::Hash,
        /// Fraction of reviewers with low confidence
        low_confidence_fraction: f64,
    },
    /// Potential 51% attack detected
    PotentialAttack {
        /// Hash of the reviewed code
        code_hash: crate::crypto::Hash,
        /// Reviewers flagged as suspicious
        suspicious_reviewers: Vec<crate::crypto::PublicKey>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_consensus_calculation() {
        let engine = SafetyConsensusEngine::new();
        let code_hash = crate::crypto::Hash::from_bytes([0; 32]);

        // Create 5 votes: 4 safe, 1 unsafe
        let mut votes = Vec::new();
        for i in 0..4 {
            votes.push(SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Safe,
                confidence: 0.9,
                reasoning: Some("Looks safe".to_string()),
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [i; 32],
                signature: crate::crypto::Signature::placeholder(),
            });
        }
        votes.push(SafetyReviewVote {
            code_hash,
            verdict: SafetyVerdict::Unsafe,
            confidence: 0.7,
            reasoning: Some("Suspicious".to_string()),
            reviewer: Keypair::generate().public_key().clone(),
            nonce: [4; 32],
            signature: crate::crypto::Signature::placeholder(),
        });

        let consensus = engine.calculate_consensus(code_hash, votes);

        // 4/5 = 80% safe votes -> should be ApprovedStrong (>= 2/3)
        assert_eq!(consensus.decision, ConsensusDecision::ApprovedStrong);
        assert_eq!(consensus.safe_votes, 4);
        assert_eq!(consensus.unsafe_votes, 1);
    }

    #[test]
    fn test_schelling_rewards() {
        let engine = SafetyConsensusEngine::new();
        let code_hash = crate::crypto::Hash::from_bytes([0; 32]);

        let reviewer1 = Keypair::generate().public_key().clone();
        let reviewer2 = Keypair::generate().public_key().clone();
        let reviewer3 = Keypair::generate().public_key().clone();

        let votes = vec![
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Safe, // Majority
                confidence: 0.9,
                reasoning: None,
                reviewer: reviewer1,
                nonce: [0; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Safe, // Majority
                confidence: 0.85,
                reasoning: None,
                reviewer: reviewer3,
                nonce: [2; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Unsafe, // Minority
                confidence: 0.8,
                reasoning: None,
                reviewer: reviewer2,
                nonce: [1; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
        ];

        let consensus = SafetyConsensus::from_votes(code_hash, votes);
        let rewards = engine.calculate_schelling_rewards(&consensus);

        // Majority voters should get more than minority voter
        assert!(rewards[0].1 > rewards[2].1);
        assert!(rewards[1].1 > rewards[2].1);
    }
}
