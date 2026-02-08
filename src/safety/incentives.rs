//! Economic incentives for safety reviewers.

use crate::crypto::PublicKey;
use crate::types::review::{
    ConsensusDecision, ReviewerReputation, SafetyConsensus, SafetyReviewVote, SafetyVerdict,
};
use std::collections::HashMap;

/// Reviewer incentive calculator
pub struct ReviewerIncentives {
    /// Base gas fee for reviewers (as fraction of total gas, 0.0 to 1.0)
    pub base_reviewer_fee: f64,
    /// Bonus multiplier for catching malicious code
    pub malicious_catch_bonus: f64,
    /// Penalty for being outlier (< 1/6 consensus)
    pub outlier_penalty: f64,
}

impl ReviewerIncentives {
    /// Create a new incentive calculator
    pub fn new() -> Self {
        Self {
            base_reviewer_fee: 0.1,     // 10% of gas goes to reviewers
            malicious_catch_bonus: 2.0, // 2x bonus for rejecting bad code
            outlier_penalty: 0.05,      // 5% fee reduction per outlier event
        }
    }

    /// Calculate payouts for all parties involved in a safety review
    pub fn calculate_payouts(
        &self,
        consensus: &SafetyConsensus,
        total_gas: u64,
        reputations: &HashMap<PublicKey, ReviewerReputation>,
    ) -> ReviewPayouts {
        let mut payouts = ReviewPayouts {
            total_gas,
            decision: consensus.decision,
            reviewer_payouts: Vec::new(),
            submitter_refund: 0,
            burned: 0,
        };

        // Calculate base amounts based on decision
        let reviewer_payout_mult = consensus.decision.reviewer_payout_multiplier();

        // Total gas allocated to reviewers
        let reviewer_pool =
            (total_gas as f64 * self.base_reviewer_fee * reviewer_payout_mult) as u64;

        // Calculate individual reviewer payouts
        let total_weight = consensus
            .votes
            .iter()
            .map(|v| self.calculate_reviewer_weight(v, consensus, reputations))
            .sum::<f64>();

        for vote in &consensus.votes {
            let weight = self.calculate_reviewer_weight(vote, consensus, reputations);
            let base_payout = if total_weight > 0.0 {
                (reviewer_pool as f64 * (weight / total_weight)) as u64
            } else {
                0
            };

            // Apply reputation multiplier
            let reputation_mult = reputations
                .get(&vote.reviewer)
                .map_or(1.0, ReviewerReputation::effective_payout_multiplier);

            let final_payout = (base_payout as f64 * reputation_mult) as u64;

            payouts.reviewer_payouts.push(ReviewerPayout {
                reviewer: vote.reviewer.clone(),
                amount: final_payout,
                was_in_majority: self.was_in_majority(vote, consensus),
                reputation_multiplier: reputation_mult,
            });
        }

        // Calculate submitter refund and burned amounts
        let total_reviewer_payout: u64 = payouts.reviewer_payouts.iter().map(|p| p.amount).sum();

        match consensus.decision {
            ConsensusDecision::ApprovedStrong | ConsensusDecision::ApprovedWeak => {
                // Code approved: small fee to reviewers, rest refunded
                payouts.submitter_refund = total_gas - total_reviewer_payout;
                payouts.burned = 0;
            }
            ConsensusDecision::RejectedWeak => {
                // Weak rejection: standard penalty (1x gas)
                payouts.submitter_refund = 0;
                payouts.burned = total_gas - total_reviewer_payout;
            }
            ConsensusDecision::RejectedStrong => {
                // Strong rejection: double penalty (2x gas withheld, but only 1x available)
                // Reviewers get their share, rest is burned
                payouts.submitter_refund = 0;
                payouts.burned = total_gas - total_reviewer_payout;
            }
            ConsensusDecision::NoConsensus => {
                // No clear consensus: partial refund
                payouts.submitter_refund = (total_gas as f64 * 0.5) as u64;
                payouts.burned = total_gas - total_reviewer_payout - payouts.submitter_refund;
            }
            ConsensusDecision::InsufficientVotes => {
                // Not enough reviewers: full refund
                payouts.submitter_refund = total_gas;
                payouts.burned = 0;
            }
        }

        payouts
    }

    /// Calculate weight for a reviewer's vote
    fn calculate_reviewer_weight(
        &self,
        vote: &SafetyReviewVote,
        consensus: &SafetyConsensus,
        reputations: &HashMap<PublicKey, ReviewerReputation>,
    ) -> f64 {
        let mut weight = 1.0;

        // Confidence weighting
        weight *= vote.confidence;

        // Majority bonus (Schelling point incentive)
        if self.was_in_majority(vote, consensus) {
            weight *= 1.5;
        } else {
            weight *= 0.5; // Minority penalty
        }

        // Reputation weighting
        if let Some(rep) = reputations.get(&vote.reviewer) {
            let trust = rep.trust_score();
            weight *= 0.5 + trust; // 0.5 to 1.5 multiplier based on trust
        }

        weight
    }

    /// Check if vote was in the majority
    fn was_in_majority(&self, vote: &SafetyReviewVote, consensus: &SafetyConsensus) -> bool {
        match consensus.decision {
            ConsensusDecision::ApprovedStrong | ConsensusDecision::ApprovedWeak => {
                vote.verdict == SafetyVerdict::Safe
            }
            ConsensusDecision::RejectedStrong | ConsensusDecision::RejectedWeak => {
                vote.verdict == SafetyVerdict::Unsafe
            }
            _ => true, // No clear majority
        }
    }

    /// Calculate expected earnings for a reviewer
    ///
    /// This helps reviewers estimate their income from participation
    pub fn estimate_earnings(
        &self,
        gas_amount: u64,
        num_reviewers: usize,
        reputation_mult: f64,
    ) -> EstimatedEarnings {
        // Scenarios with different outcomes
        let scenarios = vec![
            ("Approved (majority vote)", 0.1, true),
            ("Approved (minority vote)", 0.1, false),
            ("Rejected weak (majority)", 1.5, true),
            ("Rejected weak (minority)", 1.5, false),
            ("Rejected strong (majority)", 2.0, true),
            ("Rejected strong (minority)", 2.0, false),
        ];

        let mut earnings = EstimatedEarnings {
            scenarios: Vec::new(),
            expected_value: 0.0,
        };

        for (scenario, payout_mult, is_majority) in scenarios {
            let base_pool = (gas_amount as f64 * self.base_reviewer_fee * payout_mult) as u64;
            let per_reviewer = base_pool / num_reviewers as u64;

            let majority_mult = if is_majority { 1.5 } else { 0.5 };
            let final_payout = (per_reviewer as f64 * majority_mult * reputation_mult) as u64;

            earnings
                .scenarios
                .push((scenario.to_string(), final_payout));
        }

        // Simple expected value (assuming equal probability of each scenario)
        earnings.expected_value = earnings
            .scenarios
            .iter()
            .map(|(_, amount)| *amount as f64)
            .sum::<f64>()
            / earnings.scenarios.len() as f64;

        earnings
    }
}

impl Default for ReviewerIncentives {
    fn default() -> Self {
        Self::new()
    }
}

/// Payout distribution for a safety review
#[derive(Debug, Clone)]
pub struct ReviewPayouts {
    /// Total gas provided
    pub total_gas: u64,
    /// Consensus decision
    pub decision: ConsensusDecision,
    /// Individual reviewer payouts
    pub reviewer_payouts: Vec<ReviewerPayout>,
    /// Amount refunded to submitter
    pub submitter_refund: u64,
    /// Amount burned (penalty)
    pub burned: u64,
}

/// Individual reviewer payout
#[derive(Debug, Clone)]
pub struct ReviewerPayout {
    /// Reviewer's public key
    pub reviewer: PublicKey,
    /// Amount earned
    pub amount: u64,
    /// Whether reviewer was in majority
    pub was_in_majority: bool,
    /// Reputation multiplier applied
    pub reputation_multiplier: f64,
}

/// Estimated earnings for a reviewer
#[derive(Debug, Clone)]
pub struct EstimatedEarnings {
    /// Different scenarios and their payouts
    pub scenarios: Vec<(String, u64)>,
    /// Expected value across all scenarios
    pub expected_value: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_approved_code_payouts() {
        let incentives = ReviewerIncentives::new();
        let code_hash = crate::crypto::Hash::from_bytes([0; 32]);

        // 3 safe votes, 1 unsafe vote
        let votes = vec![
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Safe,
                confidence: 0.9,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [0; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Safe,
                confidence: 0.8,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [1; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Safe,
                confidence: 0.85,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [2; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Unsafe,
                confidence: 0.6,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [3; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
        ];

        let consensus = SafetyConsensus::from_votes(code_hash, votes);
        let payouts = incentives.calculate_payouts(&consensus, 1000, &HashMap::new());

        // Code approved, so small fee to reviewers, most refunded
        assert!(payouts.submitter_refund > 900); // >90% refunded
        assert_eq!(payouts.burned, 0); // Nothing burned for approved code
    }

    #[test]
    fn test_rejected_code_payouts() {
        let incentives = ReviewerIncentives::new();
        let code_hash = crate::crypto::Hash::from_bytes([0; 32]);

        // All unsafe votes
        let votes = vec![
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Unsafe,
                confidence: 0.95,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [0; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Unsafe,
                confidence: 0.9,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [1; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
            SafetyReviewVote {
                code_hash,
                verdict: SafetyVerdict::Unsafe,
                confidence: 0.92,
                reasoning: None,
                reviewer: Keypair::generate().public_key().clone(),
                nonce: [2; 32],
                signature: crate::crypto::Signature::placeholder(),
            },
        ];

        let consensus = SafetyConsensus::from_votes(code_hash, votes);
        let payouts = incentives.calculate_payouts(&consensus, 1000, &HashMap::new());

        // Code rejected, so no refund
        assert_eq!(payouts.submitter_refund, 0);
        // Reviewers get paid from penalty
        assert!(
            payouts
                .reviewer_payouts
                .iter()
                .map(|p| p.amount)
                .sum::<u64>()
                > 0
        );
    }

    #[test]
    fn test_earnings_estimation() {
        let incentives = ReviewerIncentives::new();

        let estimate = incentives.estimate_earnings(1000, 5, 1.0);

        // Should have multiple scenarios
        assert!(!estimate.scenarios.is_empty());
        // Expected value should be positive
        assert!(estimate.expected_value > 0.0);
    }
}
