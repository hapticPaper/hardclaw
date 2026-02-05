//! AI safety review types for verification code.

use crate::crypto::{Hash, PublicKey, Signature};
use serde::{Deserialize, Serialize};

/// AI safety review verdict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyVerdict {
    /// Code is safe to execute
    Safe,
    /// Code poses potential security risks
    Unsafe,
    /// Unable to determine (abstain from voting)
    Uncertain,
}

impl SafetyVerdict {
    /// Convert to numeric score for consensus calculation
    pub fn to_score(&self) -> i8 {
        match self {
            Self::Safe => 1,
            Self::Unsafe => -1,
            Self::Uncertain => 0,
        }
    }
}

/// Encrypted safety review vote (commit phase)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyReviewCommit {
    /// Hash of the vote (commitment)
    pub vote_hash: Hash,
    /// Reviewer's public key
    pub reviewer: PublicKey,
    /// Signature over vote_hash
    pub signature: Signature,
    /// Timestamp of submission
    pub timestamp: i64,
}

/// Revealed safety review vote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyReviewVote {
    /// Code hash being reviewed
    pub code_hash: Hash,
    /// Reviewer's verdict
    pub verdict: SafetyVerdict,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// Optional reasoning (for transparency/appeals)
    pub reasoning: Option<String>,
    /// Reviewer's public key
    pub reviewer: PublicKey,
    /// Nonce used in commit (for reveal verification)
    pub nonce: [u8; 32],
    /// Signature over entire vote
    pub signature: Signature,
}

impl SafetyReviewVote {
    /// Calculate commitment hash for this vote
    pub fn commitment_hash(&self) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(self.code_hash.as_bytes());
        data.push(self.verdict.to_score() as u8);
        data.extend_from_slice(&self.confidence.to_le_bytes());
        data.extend_from_slice(&self.nonce);
        crate::crypto::hash_data(&data)
    }

    /// Verify this vote matches its commitment
    pub fn verify_commitment(&self, commit: &SafetyReviewCommit) -> bool {
        self.commitment_hash() == commit.vote_hash && self.reviewer == commit.reviewer
    }
}

/// Consensus result for a safety review
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConsensus {
    /// Code hash that was reviewed
    pub code_hash: Hash,
    /// Total number of reviewers
    pub total_reviewers: usize,
    /// Number of Safe votes
    pub safe_votes: usize,
    /// Number of Unsafe votes
    pub unsafe_votes: usize,
    /// Number of Uncertain votes
    pub uncertain_votes: usize,
    /// Final decision
    pub decision: ConsensusDecision,
    /// Average confidence of votes
    pub avg_confidence: f64,
    /// All votes (for transparency)
    pub votes: Vec<SafetyReviewVote>,
}

impl SafetyConsensus {
    /// Calculate consensus from votes
    pub fn from_votes(code_hash: Hash, votes: Vec<SafetyReviewVote>) -> Self {
        let total = votes.len();
        let safe = votes.iter().filter(|v| v.verdict == SafetyVerdict::Safe).count();
        let unsafe_count = votes.iter().filter(|v| v.verdict == SafetyVerdict::Unsafe).count();
        let uncertain = votes.iter().filter(|v| v.verdict == SafetyVerdict::Uncertain).count();

        let avg_confidence = if total > 0 {
            votes.iter().map(|v| v.confidence).sum::<f64>() / total as f64
        } else {
            0.0
        };

        let decision = Self::calculate_decision(total, safe, unsafe_count);

        Self {
            code_hash,
            total_reviewers: total,
            safe_votes: safe,
            unsafe_votes: unsafe_count,
            uncertain_votes: uncertain,
            decision,
            avg_confidence,
            votes,
        }
    }

    /// Calculate the consensus decision based on vote counts
    fn calculate_decision(total: usize, safe: usize, unsafe_count: usize) -> ConsensusDecision {
        if total == 0 {
            return ConsensusDecision::InsufficientVotes;
        }

        let unsafe_ratio = unsafe_count as f64 / total as f64;
        let safe_ratio = safe as f64 / total as f64;

        // >= 2/3 unsafe → Rejected with double penalty
        if unsafe_ratio >= 2.0 / 3.0 {
            ConsensusDecision::RejectedStrong
        }
        // >= 1/2 unsafe → Rejected with standard penalty
        else if unsafe_ratio >= 0.5 {
            ConsensusDecision::RejectedWeak
        }
        // >= 2/3 safe → Approved
        else if safe_ratio >= 2.0 / 3.0 {
            ConsensusDecision::ApprovedStrong
        }
        // >= 1/2 safe → Approved with caution
        else if safe_ratio >= 0.5 {
            ConsensusDecision::ApprovedWeak
        }
        // No clear consensus
        else {
            ConsensusDecision::NoConsensus
        }
    }

    /// Get the fraction of votes that were in the minority (outliers)
    pub fn outlier_fraction(&self) -> f64 {
        if self.total_reviewers == 0 {
            return 0.0;
        }

        let majority_votes = match self.decision {
            ConsensusDecision::ApprovedStrong | ConsensusDecision::ApprovedWeak => self.safe_votes,
            ConsensusDecision::RejectedStrong | ConsensusDecision::RejectedWeak => self.unsafe_votes,
            ConsensusDecision::NoConsensus | ConsensusDecision::InsufficientVotes => {
                return 0.0; // No clear majority
            }
        };

        let minority_votes = self.total_reviewers - majority_votes - self.uncertain_votes;
        minority_votes as f64 / self.total_reviewers as f64
    }
}

/// Final decision from safety consensus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusDecision {
    /// >= 2/3 safe votes
    ApprovedStrong,
    /// >= 1/2 safe votes
    ApprovedWeak,
    /// >= 1/2 unsafe votes
    RejectedWeak,
    /// >= 2/3 unsafe votes
    RejectedStrong,
    /// No clear majority
    NoConsensus,
    /// Not enough votes received
    InsufficientVotes,
}

impl ConsensusDecision {
    /// Is the code approved for execution?
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::ApprovedStrong | Self::ApprovedWeak)
    }

    /// Get gas penalty multiplier (1.0 = normal, 2.0 = double penalty, 0.0 = refund)
    pub fn gas_penalty_multiplier(&self) -> f64 {
        match self {
            Self::RejectedStrong => 2.0,  // Double penalty for strong rejection
            Self::RejectedWeak => 1.0,     // Standard penalty
            Self::NoConsensus => 0.5,      // Partial refund for unclear cases
            Self::ApprovedStrong | Self::ApprovedWeak => 0.0, // No penalty
            Self::InsufficientVotes => 0.0, // Full refund if not enough reviewers
        }
    }

    /// Get reviewer payout multiplier
    pub fn reviewer_payout_multiplier(&self) -> f64 {
        match self {
            Self::RejectedStrong => 2.0,  // Reviewers get double for catching bad code
            Self::RejectedWeak => 1.5,     // Bonus for rejection
            Self::ApprovedStrong | Self::ApprovedWeak => 0.1, // Small fee for approval
            Self::NoConsensus => 0.05,     // Very small fee for unclear cases
            Self::InsufficientVotes => 0.0, // No payout if not enough participation
        }
    }
}

/// Reviewer reputation and penalty tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerReputation {
    /// Reviewer's public key
    pub reviewer: PublicKey,
    /// Total reviews participated in
    pub total_reviews: u64,
    /// Number of times in majority consensus
    pub consensus_agreements: u64,
    /// Number of times in minority (<16.7% of votes)
    pub outlier_count: u64,
    /// Current penalty multiplier (0.0 to 1.0, starts at 1.0)
    pub penalty_multiplier: f64,
    /// Blocks remaining for current penalty
    pub penalty_blocks_remaining: u64,
    /// Historical accuracy score (exponential moving average)
    pub accuracy_ema: f64,
}

impl ReviewerReputation {
    /// Create new reputation for a reviewer
    pub fn new(reviewer: PublicKey) -> Self {
        Self {
            reviewer,
            total_reviews: 0,
            consensus_agreements: 0,
            outlier_count: 0,
            penalty_multiplier: 1.0,
            penalty_blocks_remaining: 0,
            accuracy_ema: 0.5, // Start neutral
        }
    }

    /// Update reputation after a review
    pub fn update_after_review(&mut self, was_in_majority: bool, outlier_fraction: f64) {
        self.total_reviews += 1;

        if was_in_majority {
            self.consensus_agreements += 1;
        }

        // Outlier detection: < 1/6 of votes (16.7%)
        if outlier_fraction < 1.0 / 6.0 {
            self.outlier_count += 1;
            // Apply penalty for next 10 reviews
            self.penalty_blocks_remaining = 10;
            self.penalty_multiplier *= 0.95; // 5% reduction per outlier
        }

        // Update accuracy EMA (exponential moving average)
        let alpha = 0.1; // Smoothing factor
        let accuracy = if was_in_majority { 1.0 } else { 0.0 };
        self.accuracy_ema = alpha * accuracy + (1.0 - alpha) * self.accuracy_ema;

        // Gradually recover penalty multiplier for good behavior
        if was_in_majority && self.penalty_blocks_remaining > 0 {
            self.penalty_blocks_remaining = self.penalty_blocks_remaining.saturating_sub(1);
            if self.penalty_blocks_remaining == 0 {
                self.penalty_multiplier = (self.penalty_multiplier * 1.05).min(1.0);
            }
        }
    }

    /// Get current effective payout multiplier
    pub fn effective_payout_multiplier(&self) -> f64 {
        if self.penalty_blocks_remaining > 0 {
            self.penalty_multiplier
        } else {
            1.0
        }
    }

    /// Calculate trust score (0.0 to 1.0)
    pub fn trust_score(&self) -> f64 {
        if self.total_reviews < 10 {
            // Not enough data yet
            return 0.5;
        }

        // Combine multiple factors
        let agreement_ratio = self.consensus_agreements as f64 / self.total_reviews as f64;
        let outlier_penalty = 1.0 - (self.outlier_count as f64 / self.total_reviews as f64).min(0.5);
        
        // Weighted average
        0.4 * agreement_ratio + 0.3 * outlier_penalty + 0.3 * self.accuracy_ema
    }
}

/// Safety review request for verification code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyReviewRequest {
    /// Hash of code to review
    pub code_hash: Hash,
    /// Actual code (for AI analysis)
    pub code: String,
    /// Programming language
    pub language: String,
    /// Submitter's address
    pub submitter: PublicKey,
    /// Gas provided for review
    pub gas_amount: u64,
    /// Deadline for review completion
    pub deadline: i64,
    /// Minimum number of reviewers required
    pub min_reviewers: usize,
}

/// Safety review session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyReviewSession {
    /// Review request
    pub request: SafetyReviewRequest,
    /// Selected reviewers (randomly chosen)
    pub selected_reviewers: Vec<PublicKey>,
    /// Commits received (commit phase)
    pub commits: Vec<SafetyReviewCommit>,
    /// Votes revealed (reveal phase)
    pub votes: Vec<SafetyReviewVote>,
    /// Current phase
    pub phase: ReviewPhase,
    /// Commit phase deadline
    pub commit_deadline: i64,
    /// Reveal phase deadline
    pub reveal_deadline: i64,
}

/// Review process phases
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewPhase {
    /// Waiting for reviewers to be selected
    Selection,
    /// Reviewers submit commitments (encrypted votes)
    Commit,
    /// Reviewers reveal their votes
    Reveal,
    /// Consensus calculated, payouts distributed
    Complete,
    /// Review expired without enough participation
    Expired,
}
