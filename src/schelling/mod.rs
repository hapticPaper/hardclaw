//! Schelling Point Consensus for Subjective Tasks.
//!
//! Not all tasks are mathematically deterministic (e.g., "Write a funny poem").
//! For these, `HardClaw` uses Schelling Point consensus.
//!
//! ## The Subjective Verification Layer
//!
//! For tasks tagged `type: subjective`:
//! 1. **Redundancy**: The job is sent to 5 independent Solvers
//! 2. **Blind Voting**: Miners function as "Jurors", hashing their vote
//! 3. **Reveal**: Once block is proposed, votes are revealed
//! 4. **Reward**: Miners who voted with majority receive reward; deviants are slashed

mod voting;
mod quality;

pub use voting::{SchellingVoting, VotingPhase, VotingRound};
pub use quality::{QualityAssessment, QualityMetric};

use std::collections::HashMap;

use crate::crypto::PublicKey;
use crate::types::{
    Id, Timestamp, VerificationVote, VoteResult,
    VotingResults, now_millis,
};

/// Configuration for Schelling Point consensus
#[derive(Clone, Debug)]
pub struct SchellingConfig {
    /// Number of solvers for redundancy
    pub solver_redundancy: usize,
    /// Minimum voters required
    pub min_voters: usize,
    /// Commit phase duration (milliseconds)
    pub commit_phase_ms: i64,
    /// Reveal phase duration (milliseconds)
    pub reveal_phase_ms: i64,
    /// Quality threshold for acceptance (0-100)
    pub quality_threshold: u8,
    /// Slash percentage for voting against majority
    pub deviant_slash_percent: u8,
}

impl Default for SchellingConfig {
    fn default() -> Self {
        Self {
            solver_redundancy: 5,
            min_voters: 3,
            commit_phase_ms: 30_000,  // 30 seconds
            reveal_phase_ms: 30_000,  // 30 seconds
            quality_threshold: 70,
            deviant_slash_percent: 5,
        }
    }
}

/// Manages Schelling Point consensus rounds
pub struct SchellingConsensus {
    /// Configuration
    config: SchellingConfig,
    /// Active voting rounds by solution ID
    active_rounds: HashMap<Id, VotingRound>,
    /// Completed rounds (for history/appeals)
    completed_rounds: HashMap<Id, CompletedRound>,
}

impl Default for SchellingConsensus {
    fn default() -> Self {
        Self::new(SchellingConfig::default())
    }
}

impl SchellingConsensus {
    /// Create new Schelling consensus manager
    #[must_use]
    pub fn new(config: SchellingConfig) -> Self {
        Self {
            config,
            active_rounds: HashMap::new(),
            completed_rounds: HashMap::new(),
        }
    }

    /// Start a voting round for a solution
    pub fn start_round(&mut self, solution_id: Id) -> Result<&VotingRound, SchellingError> {
        if self.active_rounds.contains_key(&solution_id) {
            return Err(SchellingError::RoundAlreadyExists);
        }

        let round = VotingRound::new(
            solution_id,
            self.config.commit_phase_ms,
            self.config.reveal_phase_ms,
        );

        self.active_rounds.insert(solution_id, round);
        Ok(self.active_rounds.get(&solution_id).expect("just inserted"))
    }

    /// Submit a vote commitment
    pub fn submit_commitment(
        &mut self,
        solution_id: &Id,
        vote: VerificationVote,
    ) -> Result<(), SchellingError> {
        let round = self.active_rounds.get_mut(solution_id)
            .ok_or(SchellingError::RoundNotFound)?;

        if round.phase() != VotingPhase::Commit {
            return Err(SchellingError::WrongPhase {
                expected: VotingPhase::Commit,
                actual: round.phase(),
            });
        }

        round.add_commitment(vote)
    }

    /// Reveal a vote
    pub fn reveal_vote(
        &mut self,
        solution_id: &Id,
        voter: &PublicKey,
        vote: VoteResult,
        quality_score: u8,
        nonce: [u8; 32],
    ) -> Result<(), SchellingError> {
        let round = self.active_rounds.get_mut(solution_id)
            .ok_or(SchellingError::RoundNotFound)?;

        if round.phase() != VotingPhase::Reveal {
            return Err(SchellingError::WrongPhase {
                expected: VotingPhase::Reveal,
                actual: round.phase(),
            });
        }

        round.reveal_vote(voter, vote, quality_score, nonce)
    }

    /// Finalize a round and determine outcome
    pub fn finalize_round(&mut self, solution_id: &Id) -> Result<RoundOutcome, SchellingError> {
        let round = self.active_rounds.remove(solution_id)
            .ok_or(SchellingError::RoundNotFound)?;

        if round.phase() != VotingPhase::Complete {
            return Err(SchellingError::RoundNotComplete);
        }

        let results = round.tally_votes();

        // Determine outcome
        let accepted = results.majority == Some(VoteResult::Accept)
            && results.avg_quality_score >= f64::from(self.config.quality_threshold);

        // Identify deviants (voted against majority)
        let deviants: Vec<PublicKey> = round.votes.iter()
            .filter_map(|(voter, vote)| {
                if let Some(v) = vote.vote {
                    if results.majority.map(|m| v != m && v != VoteResult::Abstain).unwrap_or(false) {
                        return Some(*voter);
                    }
                }
                None
            })
            .collect();

        let outcome = RoundOutcome {
            solution_id: *solution_id,
            accepted,
            results,
            deviants,
            finalized_at: now_millis(),
        };

        // Store completed round
        self.completed_rounds.insert(*solution_id, CompletedRound {
            round,
            outcome: outcome.clone(),
        });

        Ok(outcome)
    }

    /// Get active round for a solution
    #[must_use]
    pub fn get_round(&self, solution_id: &Id) -> Option<&VotingRound> {
        self.active_rounds.get(solution_id)
    }

    /// Process time-based phase transitions
    pub fn tick(&mut self) {
        for round in self.active_rounds.values_mut() {
            round.check_phase_transition();
        }
    }

    /// Get configuration
    #[must_use]
    pub const fn config(&self) -> &SchellingConfig {
        &self.config
    }
}

/// Outcome of a completed voting round
#[derive(Clone, Debug)]
pub struct RoundOutcome {
    /// Solution that was voted on
    pub solution_id: Id,
    /// Whether the solution was accepted
    pub accepted: bool,
    /// Voting results
    pub results: VotingResults,
    /// Voters who deviated from majority (to be slashed)
    pub deviants: Vec<PublicKey>,
    /// When the round was finalized
    pub finalized_at: Timestamp,
}

/// A completed voting round (for history)
#[derive(Clone, Debug)]
pub struct CompletedRound {
    /// The voting round
    pub round: VotingRound,
    /// The outcome
    pub outcome: RoundOutcome,
}

/// Schelling consensus errors
#[derive(Debug, thiserror::Error)]
pub enum SchellingError {
    /// Round already exists for this solution
    #[error("voting round already exists")]
    RoundAlreadyExists,
    /// Round not found
    #[error("voting round not found")]
    RoundNotFound,
    /// Wrong phase for operation
    #[error("wrong phase: expected {expected:?}, got {actual:?}")]
    WrongPhase {
        /// Expected voting phase
        expected: VotingPhase,
        /// Actual voting phase
        actual: VotingPhase,
    },
    /// Round not yet complete
    #[error("voting round not complete")]
    RoundNotComplete,
    /// Voter not found
    #[error("voter not found")]
    VoterNotFound,
    /// Commitment verification failed
    #[error("commitment verification failed")]
    CommitmentMismatch,
    /// Duplicate vote
    #[error("duplicate vote from this voter")]
    DuplicateVote,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{Hash, Keypair};

    #[test]
    fn test_schelling_round_lifecycle() {
        let mut consensus = SchellingConsensus::new(SchellingConfig {
            commit_phase_ms: 0, // Instant for testing
            reveal_phase_ms: 0,
            ..Default::default()
        });

        let solution_id = Hash::ZERO;

        // Start round
        consensus.start_round(solution_id).unwrap();

        // Submit commitments and store nonces for reveal phase
        let voters: Vec<Keypair> = (0..5).map(|_| Keypair::generate()).collect();
        let mut nonces: Vec<[u8; 32]> = Vec::new();

        for (i, voter_kp) in voters.iter().enumerate() {
            let vote = if i < 4 {
                VerificationVote::commit(solution_id, *voter_kp.public_key(), VoteResult::Accept, 80)
            } else {
                VerificationVote::commit(solution_id, *voter_kp.public_key(), VoteResult::Reject, 30)
            };
            // Store the nonce before submitting (submission clears it)
            nonces.push(vote.nonce.unwrap());
            consensus.submit_commitment(&solution_id, vote).unwrap();
        }

        // Transition to reveal phase
        {
            let round = consensus.active_rounds.get_mut(&solution_id).unwrap();
            round.force_phase(VotingPhase::Reveal);
        }

        // Reveal votes using stored nonces
        for (i, voter_kp) in voters.iter().enumerate() {
            let (vote, quality) = if i < 4 {
                (VoteResult::Accept, 80)
            } else {
                (VoteResult::Reject, 30)
            };

            consensus.reveal_vote(
                &solution_id,
                voter_kp.public_key(),
                vote,
                quality,
                nonces[i],
            ).unwrap();
        }

        // Transition to complete
        {
            let round = consensus.active_rounds.get_mut(&solution_id).unwrap();
            round.force_phase(VotingPhase::Complete);
        }

        // Finalize
        let outcome = consensus.finalize_round(&solution_id).unwrap();

        assert!(outcome.accepted);
        assert_eq!(outcome.results.accept_votes, 4);
        assert_eq!(outcome.results.reject_votes, 1);
        assert_eq!(outcome.deviants.len(), 1); // The rejector
    }
}
