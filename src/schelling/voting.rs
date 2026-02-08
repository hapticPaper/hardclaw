//! Voting mechanics for Schelling Point consensus.

use std::collections::HashMap;

use crate::crypto::PublicKey;
use crate::types::{now_millis, Id, Timestamp, VerificationVote, VoteResult, VotingResults};

use super::SchellingError;

/// Phase of a voting round
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VotingPhase {
    /// Accepting vote commitments
    Commit,
    /// Accepting vote reveals
    Reveal,
    /// Voting complete, ready for finalization
    Complete,
}

/// A Schelling Point voting round
#[derive(Clone, Debug)]
pub struct VotingRound {
    /// Solution being voted on
    pub solution_id: Id,
    /// Current phase
    phase: VotingPhase,
    /// When commit phase started
    pub commit_start: Timestamp,
    /// When reveal phase starts
    pub reveal_start: Timestamp,
    /// When round ends
    pub end_time: Timestamp,
    /// Votes by voter public key
    pub votes: HashMap<PublicKey, VerificationVote>,
}

impl VotingRound {
    /// Create a new voting round
    #[must_use]
    pub fn new(solution_id: Id, commit_duration_ms: i64, reveal_duration_ms: i64) -> Self {
        let now = now_millis();

        Self {
            solution_id,
            phase: VotingPhase::Commit,
            commit_start: now,
            reveal_start: now + commit_duration_ms,
            end_time: now + commit_duration_ms + reveal_duration_ms,
            votes: HashMap::new(),
        }
    }

    /// Get current phase
    #[must_use]
    pub const fn phase(&self) -> VotingPhase {
        self.phase
    }

    /// Check and perform phase transition if needed
    pub fn check_phase_transition(&mut self) {
        let now = now_millis();

        match self.phase {
            VotingPhase::Commit => {
                if now >= self.reveal_start {
                    self.phase = VotingPhase::Reveal;
                }
            }
            VotingPhase::Reveal => {
                if now >= self.end_time {
                    self.phase = VotingPhase::Complete;
                }
            }
            VotingPhase::Complete => {}
        }
    }

    /// Force a phase transition (for testing)
    #[cfg(test)]
    pub fn force_phase(&mut self, phase: VotingPhase) {
        self.phase = phase;
    }

    /// Add a vote commitment
    pub fn add_commitment(&mut self, vote: VerificationVote) -> Result<(), SchellingError> {
        if self.votes.contains_key(&vote.voter) {
            return Err(SchellingError::DuplicateVote);
        }

        // Store only the public commitment (no revealed values)
        let voter = vote.voter.clone();
        self.votes.insert(voter, vote.public_commitment());
        Ok(())
    }

    /// Reveal a vote
    pub fn reveal_vote(
        &mut self,
        voter: &PublicKey,
        vote: VoteResult,
        quality_score: u8,
        nonce: [u8; 32],
    ) -> Result<(), SchellingError> {
        let commitment = self
            .votes
            .get_mut(voter)
            .ok_or(SchellingError::VoterNotFound)?;

        commitment
            .reveal(vote, quality_score, nonce)
            .map_err(|_| SchellingError::CommitmentMismatch)?;

        Ok(())
    }

    /// Tally the votes
    #[must_use]
    pub fn tally_votes(&self) -> VotingResults {
        let votes_owned: Vec<VerificationVote> = self
            .votes
            .values()
            .filter(|v| v.is_revealed())
            .cloned()
            .collect();

        VotingResults::from_votes(&votes_owned)
    }

    /// Get number of commitments
    #[must_use]
    pub fn commitment_count(&self) -> usize {
        self.votes.len()
    }

    /// Get number of reveals
    #[must_use]
    pub fn reveal_count(&self) -> usize {
        self.votes.values().filter(|v| v.is_revealed()).count()
    }

    /// Check if minimum voters threshold is met
    #[must_use]
    pub fn has_quorum(&self, min_voters: usize) -> bool {
        self.reveal_count() >= min_voters
    }
}

/// Manages multiple concurrent voting rounds
pub struct SchellingVoting {
    /// Active rounds
    rounds: HashMap<Id, VotingRound>,
    /// Commit phase duration
    commit_duration_ms: i64,
    /// Reveal phase duration
    reveal_duration_ms: i64,
}

impl Default for SchellingVoting {
    fn default() -> Self {
        Self::new(30_000, 30_000)
    }
}

impl SchellingVoting {
    /// Create new voting manager
    #[must_use]
    pub fn new(commit_duration_ms: i64, reveal_duration_ms: i64) -> Self {
        Self {
            rounds: HashMap::new(),
            commit_duration_ms,
            reveal_duration_ms,
        }
    }

    /// Start a new voting round
    pub fn start_round(&mut self, solution_id: Id) -> &VotingRound {
        let round = VotingRound::new(
            solution_id,
            self.commit_duration_ms,
            self.reveal_duration_ms,
        );

        self.rounds.insert(solution_id, round);
        self.rounds.get(&solution_id).expect("just inserted")
    }

    /// Get a round by solution ID
    #[must_use]
    pub fn get_round(&self, solution_id: &Id) -> Option<&VotingRound> {
        self.rounds.get(solution_id)
    }

    /// Get a mutable round by solution ID
    pub fn get_round_mut(&mut self, solution_id: &Id) -> Option<&mut VotingRound> {
        self.rounds.get_mut(solution_id)
    }

    /// Remove a completed round
    pub fn remove_round(&mut self, solution_id: &Id) -> Option<VotingRound> {
        self.rounds.remove(solution_id)
    }

    /// Update all round phases
    pub fn tick(&mut self) {
        for round in self.rounds.values_mut() {
            round.check_phase_transition();
        }
    }

    /// Get rounds in a specific phase
    #[must_use]
    pub fn rounds_in_phase(&self, phase: VotingPhase) -> Vec<&VotingRound> {
        self.rounds
            .values()
            .filter(|r| r.phase() == phase)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{Hash, Keypair};

    #[test]
    fn test_voting_round_creation() {
        let round = VotingRound::new(Hash::ZERO, 1000, 1000);

        assert_eq!(round.phase(), VotingPhase::Commit);
        assert_eq!(round.commitment_count(), 0);
    }

    #[test]
    fn test_commitment_and_reveal() {
        let mut round = VotingRound::new(Hash::ZERO, 1000, 1000);
        let voter = Keypair::generate();

        // Commit
        let vote =
            VerificationVote::commit(Hash::ZERO, voter.public_key().clone(), VoteResult::Accept, 85);
        let nonce = vote.nonce.unwrap();

        round.add_commitment(vote).unwrap();
        assert_eq!(round.commitment_count(), 1);

        // Move to reveal phase
        round.force_phase(VotingPhase::Reveal);

        // Reveal
        round
            .reveal_vote(voter.public_key(), VoteResult::Accept, 85, nonce)
            .unwrap();
        assert_eq!(round.reveal_count(), 1);
    }

    #[test]
    fn test_duplicate_vote_rejected() {
        let mut round = VotingRound::new(Hash::ZERO, 1000, 1000);
        let voter = Keypair::generate();

        let first_vote =
            VerificationVote::commit(Hash::ZERO, voter.public_key().clone(), VoteResult::Accept, 85);
        let second_vote =
            VerificationVote::commit(Hash::ZERO, voter.public_key().clone(), VoteResult::Reject, 30);

        round.add_commitment(first_vote).unwrap();
        assert!(matches!(
            round.add_commitment(second_vote),
            Err(SchellingError::DuplicateVote)
        ));
    }

    #[test]
    fn test_tally_votes() {
        let mut round = VotingRound::new(Hash::ZERO, 0, 0);

        // Add 3 accept, 2 reject
        for i in 0..5 {
            let voter = Keypair::generate();
            let (vote_result, quality) = if i < 3 {
                (VoteResult::Accept, 80)
            } else {
                (VoteResult::Reject, 40)
            };

            let vote =
                VerificationVote::commit(Hash::ZERO, voter.public_key().clone(), vote_result, quality);
            let nonce = vote.nonce.unwrap();
            round.add_commitment(vote).unwrap();

            // Immediately reveal for test
            round.force_phase(VotingPhase::Reveal);
            round
                .reveal_vote(voter.public_key(), vote_result, quality, nonce)
                .unwrap();
            round.force_phase(VotingPhase::Commit);
        }

        round.force_phase(VotingPhase::Complete);
        let results = round.tally_votes();

        assert_eq!(results.accept_votes, 3);
        assert_eq!(results.reject_votes, 2);
        assert_eq!(results.majority, Some(VoteResult::Accept));
    }
}
