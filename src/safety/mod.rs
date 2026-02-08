//! AI-powered safety review for verification code.
//!
//! This module provides a crypto-economic system for validators to review
//! verification code using AI models before allowing execution.

pub mod ai_review;
pub mod consensus;
pub mod incentives;

pub use ai_review::AIReviewer;
pub use consensus::SafetyConsensusEngine;
pub use incentives::ReviewerIncentives;

use crate::crypto::PublicKey;
use crate::types::review::{
    ConsensusDecision, ReviewPhase, ReviewerReputation, SafetyReviewCommit, SafetyReviewRequest,
    SafetyReviewSession, SafetyReviewVote, SafetyVerdict,
};
use std::collections::HashMap;

/// Safety review manager
pub struct SafetyReviewManager {
    /// Active review sessions
    sessions: HashMap<crate::crypto::Hash, SafetyReviewSession>,
    /// Reviewer reputations
    reputations: HashMap<PublicKey, ReviewerReputation>,
    /// Consensus engine
    consensus: SafetyConsensusEngine,
    /// Incentive calculator
    incentives: ReviewerIncentives,
}

impl SafetyReviewManager {
    /// Create a new safety review manager
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            reputations: HashMap::new(),
            consensus: SafetyConsensusEngine::new(),
            incentives: ReviewerIncentives::new(),
        }
    }

    /// Start a new safety review session
    pub fn start_review(
        &mut self,
        request: SafetyReviewRequest,
        available_reviewers: &[PublicKey],
    ) -> Result<SafetyReviewSession, String> {
        // Select reviewers randomly but weighted by reputation
        let selected = self.select_reviewers(available_reviewers, request.min_reviewers)?;

        let now = crate::types::now_millis();
        let session = SafetyReviewSession {
            request: request.clone(),
            selected_reviewers: selected,
            commits: Vec::new(),
            votes: Vec::new(),
            phase: ReviewPhase::Commit,
            commit_deadline: now + 300_000, // 5 minutes for commits
            reveal_deadline: now + 600_000, // Additional 5 minutes for reveals
        };

        self.sessions.insert(request.code_hash, session.clone());
        Ok(session)
    }

    /// Select reviewers weighted by reputation
    fn select_reviewers(
        &self,
        available: &[PublicKey],
        count: usize,
    ) -> Result<Vec<PublicKey>, String> {
        if available.len() < count {
            return Err("Not enough available reviewers".to_string());
        }

        // Calculate weights based on trust scores
        let mut weighted: Vec<(PublicKey, f64)> = available
            .iter()
            .map(|pk| {
                let trust = self
                    .reputations
                    .get(pk)
                    .map_or(0.5, ReviewerReputation::trust_score); // New reviewers start at 0.5
                (pk.clone(), trust)
            })
            .collect();

        // Simple weighted random selection (in production, use VRF for verifiability)
        weighted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(weighted
            .iter()
            .take(count)
            .map(|(pk, _)| pk.clone())
            .collect())
    }

    /// Submit a vote commitment
    pub fn submit_commit(
        &mut self,
        code_hash: crate::crypto::Hash,
        commit: SafetyReviewCommit,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&code_hash)
            .ok_or("Review session not found")?;

        if session.phase != ReviewPhase::Commit {
            return Err("Not in commit phase".to_string());
        }

        let now = crate::types::now_millis();
        if now > session.commit_deadline {
            session.phase = ReviewPhase::Reveal;
            return Err("Commit phase expired".to_string());
        }

        // Verify reviewer is selected
        if !session.selected_reviewers.contains(&commit.reviewer) {
            return Err("Reviewer not selected for this session".to_string());
        }

        // Check for duplicate commits
        if session
            .commits
            .iter()
            .any(|c| c.reviewer == commit.reviewer)
        {
            return Err("Reviewer already submitted commit".to_string());
        }

        session.commits.push(commit);
        Ok(())
    }

    /// Reveal a vote
    pub fn reveal_vote(
        &mut self,
        code_hash: crate::crypto::Hash,
        vote: SafetyReviewVote,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&code_hash)
            .ok_or("Review session not found")?;

        // Auto-transition to reveal phase if commit deadline passed
        let now = crate::types::now_millis();
        if session.phase == ReviewPhase::Commit && now > session.commit_deadline {
            session.phase = ReviewPhase::Reveal;
        }

        if session.phase != ReviewPhase::Reveal {
            return Err("Not in reveal phase".to_string());
        }

        if now > session.reveal_deadline {
            return self.finalize_review(code_hash);
        }

        // Find matching commit
        let commit = session
            .commits
            .iter()
            .find(|c| c.reviewer == vote.reviewer)
            .ok_or("No commit found for this reviewer")?;

        // Verify vote matches commitment
        if !vote.verify_commitment(commit) {
            return Err("Vote does not match commitment".to_string());
        }

        // Check for duplicate reveals
        if session.votes.iter().any(|v| v.reviewer == vote.reviewer) {
            return Err("Reviewer already revealed vote".to_string());
        }

        session.votes.push(vote);

        // Check if all commits have been revealed
        if session.votes.len() == session.commits.len() {
            return self.finalize_review(code_hash);
        }

        Ok(())
    }

    /// Finalize review and calculate payouts
    fn finalize_review(&mut self, code_hash: crate::crypto::Hash) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&code_hash)
            .ok_or("Review session not found")?;

        if session.votes.is_empty() {
            session.phase = ReviewPhase::Expired;
            return Err("No votes received".to_string());
        }

        // Calculate consensus
        let consensus = self
            .consensus
            .calculate_consensus(code_hash, session.votes.clone());

        // Update reputations and calculate payouts
        let _payouts = self.incentives.calculate_payouts(
            &consensus,
            session.request.gas_amount,
            &self.reputations,
        );

        // Update reviewer reputations
        for vote in &session.votes {
            let reputation = self
                .reputations
                .entry(vote.reviewer.clone())
                .or_insert_with(|| ReviewerReputation::new(vote.reviewer.clone()));

            let was_in_majority = match consensus.decision {
                ConsensusDecision::ApprovedStrong | ConsensusDecision::ApprovedWeak => {
                    vote.verdict == SafetyVerdict::Safe
                }
                ConsensusDecision::RejectedStrong | ConsensusDecision::RejectedWeak => {
                    vote.verdict == SafetyVerdict::Unsafe
                }
                _ => true, // No penalty for unclear cases
            };

            reputation.update_after_review(was_in_majority, consensus.outlier_fraction());
        }

        session.phase = ReviewPhase::Complete;

        // Store consensus result (would be persisted to blockchain/state)
        tracing::info!(
            "Safety review complete for code {:?}: {:?}",
            code_hash,
            consensus.decision
        );

        Ok(())
    }

    /// Get reviewer reputation
    pub fn get_reputation(&self, reviewer: &PublicKey) -> Option<&ReviewerReputation> {
        self.reputations.get(reviewer)
    }

    /// Get active review session
    pub fn get_session(&self, code_hash: &crate::crypto::Hash) -> Option<&SafetyReviewSession> {
        self.sessions.get(code_hash)
    }
}

impl Default for SafetyReviewManager {
    fn default() -> Self {
        Self::new()
    }
}
