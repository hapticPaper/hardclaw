//! Verification results and voting types.

use serde::{Deserialize, Serialize};

use crate::crypto::{PublicKey, Signature, Commitment};
use super::{Id, Timestamp, now_millis};

/// Result of verifying a solution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationResult {
    /// ID of the solution that was verified
    pub solution_id: Id,
    /// ID of the job
    pub job_id: Id,
    /// The verifier who performed this verification
    pub verifier: PublicKey,
    /// Whether the solution passed verification
    pub passed: bool,
    /// Optional error message if verification failed
    pub error: Option<String>,
    /// Time taken to verify (in milliseconds)
    pub verification_time_ms: u64,
    /// When the verification was completed
    pub verified_at: Timestamp,
    /// Verifier's signature over the result
    pub signature: Signature,
}

impl VerificationResult {
    /// Create a new verification result
    #[must_use]
    pub fn new(
        solution_id: Id,
        job_id: Id,
        verifier: PublicKey,
        passed: bool,
        error: Option<String>,
        verification_time_ms: u64,
    ) -> Self {
        Self {
            solution_id,
            job_id,
            verifier,
            passed,
            error,
            verification_time_ms,
            verified_at: now_millis(),
            signature: Signature::from_bytes([0u8; 64]),
        }
    }

    /// Get bytes to sign
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.solution_id.as_bytes());
        data.extend_from_slice(self.job_id.as_bytes());
        data.extend_from_slice(self.verifier.as_bytes());
        data.push(u8::from(self.passed));
        data.extend_from_slice(&self.verified_at.to_le_bytes());
        data
    }

    /// Verify the result signature
    ///
    /// # Errors
    /// Returns error if signature is invalid
    pub fn verify_signature(&self) -> Result<(), crate::crypto::CryptoError> {
        crate::crypto::verify(&self.verifier, &self.signing_bytes(), &self.signature)
    }
}

/// Result of a Schelling Point vote
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoteResult {
    /// Solution is acceptable (passes quality threshold)
    Accept,
    /// Solution is not acceptable
    Reject,
    /// Abstain from voting
    Abstain,
}

impl VoteResult {
    /// Convert to a byte for hashing
    #[must_use]
    pub const fn as_byte(&self) -> u8 {
        match self {
            Self::Accept => 1,
            Self::Reject => 2,
            Self::Abstain => 0,
        }
    }
}

impl AsRef<[u8]> for VoteResult {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Accept => &[1],
            Self::Reject => &[2],
            Self::Abstain => &[0],
        }
    }
}

/// A vote in Schelling Point consensus (commit phase)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationVote {
    /// Solution being voted on
    pub solution_id: Id,
    /// Voter's public key
    pub voter: PublicKey,
    /// Commitment to the vote (hash of vote + nonce)
    pub commitment: Commitment,
    /// The actual vote (None until reveal phase)
    pub vote: Option<VoteResult>,
    /// The nonce used (None until reveal phase)
    pub nonce: Option<[u8; 32]>,
    /// Quality score (0-100, revealed with vote)
    pub quality_score: Option<u8>,
    /// When the vote was committed
    pub committed_at: Timestamp,
    /// When the vote was revealed
    pub revealed_at: Option<Timestamp>,
    /// Signature over the commitment
    pub signature: Signature,
}

impl VerificationVote {
    /// Create a new vote commitment
    #[must_use]
    pub fn commit(
        solution_id: Id,
        voter: PublicKey,
        vote: VoteResult,
        quality_score: u8,
    ) -> Self {
        let nonce: [u8; 32] = rand::random();

        // Create commitment: hash(vote || quality_score || nonce)
        let mut vote_data = Vec::new();
        vote_data.push(vote.as_byte());
        vote_data.push(quality_score);
        vote_data.extend_from_slice(&nonce);

        let commitment = Commitment::create(&vote_data, &nonce);

        Self {
            solution_id,
            voter,
            commitment,
            vote: Some(vote),
            nonce: Some(nonce),
            quality_score: Some(quality_score),
            committed_at: now_millis(),
            revealed_at: None,
            signature: Signature::from_bytes([0u8; 64]),
        }
    }

    /// Create a commitment-only view (for broadcasting before reveal)
    #[must_use]
    pub fn public_commitment(&self) -> Self {
        Self {
            solution_id: self.solution_id,
            voter: self.voter,
            commitment: self.commitment,
            vote: None,
            nonce: None,
            quality_score: None,
            committed_at: self.committed_at,
            revealed_at: None,
            signature: self.signature,
        }
    }

    /// Reveal the vote
    ///
    /// # Errors
    /// Returns error if the reveal doesn't match the commitment
    pub fn reveal(
        &mut self,
        vote: VoteResult,
        quality_score: u8,
        nonce: [u8; 32],
    ) -> Result<(), crate::crypto::CryptoError> {
        // Verify the commitment
        let mut vote_data = Vec::new();
        vote_data.push(vote.as_byte());
        vote_data.push(quality_score);
        vote_data.extend_from_slice(&nonce);

        self.commitment.verify(&vote_data, &nonce)?;

        self.vote = Some(vote);
        self.quality_score = Some(quality_score);
        self.nonce = Some(nonce);
        self.revealed_at = Some(now_millis());

        Ok(())
    }

    /// Check if the vote has been revealed
    #[must_use]
    pub const fn is_revealed(&self) -> bool {
        self.vote.is_some()
    }

    /// Get bytes to sign (for commitment signature)
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.solution_id.as_bytes());
        data.extend_from_slice(self.voter.as_bytes());
        data.extend_from_slice(self.commitment.as_hash().as_bytes());
        data.extend_from_slice(&self.committed_at.to_le_bytes());
        data
    }
}

/// Aggregated voting results for Schelling Point consensus
#[derive(Clone, Debug, Default)]
pub struct VotingResults {
    /// Total votes cast
    pub total_votes: usize,
    /// Votes to accept
    pub accept_votes: usize,
    /// Votes to reject
    pub reject_votes: usize,
    /// Abstentions
    pub abstain_votes: usize,
    /// Average quality score from accepters
    pub avg_quality_score: f64,
    /// The majority result
    pub majority: Option<VoteResult>,
}

impl VotingResults {
    /// Create from a list of revealed votes
    #[must_use]
    pub fn from_votes(votes: &[VerificationVote]) -> Self {
        let mut results = Self::default();

        let mut quality_sum: u64 = 0;
        let mut quality_count: u64 = 0;

        for vote in votes {
            if let Some(v) = vote.vote {
                results.total_votes += 1;
                match v {
                    VoteResult::Accept => {
                        results.accept_votes += 1;
                        if let Some(q) = vote.quality_score {
                            quality_sum += u64::from(q);
                            quality_count += 1;
                        }
                    }
                    VoteResult::Reject => results.reject_votes += 1,
                    VoteResult::Abstain => results.abstain_votes += 1,
                }
            }
        }

        if quality_count > 0 {
            results.avg_quality_score = quality_sum as f64 / quality_count as f64;
        }

        // Determine majority (excluding abstentions)
        let participating = results.accept_votes + results.reject_votes;
        if participating > 0 {
            if results.accept_votes > results.reject_votes {
                results.majority = Some(VoteResult::Accept);
            } else if results.reject_votes > results.accept_votes {
                results.majority = Some(VoteResult::Reject);
            }
            // Tie = no majority
        }

        results
    }

    /// Check if there's a clear majority (> 50%)
    #[must_use]
    pub fn has_majority(&self) -> bool {
        self.majority.is_some()
    }

    /// Get the percentage of accept votes (excluding abstentions)
    #[must_use]
    pub fn accept_percentage(&self) -> f64 {
        let participating = self.accept_votes + self.reject_votes;
        if participating == 0 {
            0.0
        } else {
            self.accept_votes as f64 / participating as f64 * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{Hash, Keypair};

    #[test]
    fn test_vote_commit_reveal() {
        let kp = Keypair::generate();
        let solution_id = Hash::ZERO;

        let vote = VerificationVote::commit(
            solution_id,
            *kp.public_key(),
            VoteResult::Accept,
            85,
        );

        assert!(vote.is_revealed());

        // Create public version
        let public = vote.public_commitment();
        assert!(!public.is_revealed());

        // Reveal should work with correct values
        let mut to_reveal = public;
        assert!(to_reveal.reveal(
            VoteResult::Accept,
            85,
            vote.nonce.expect("should have nonce"),
        ).is_ok());
    }

    #[test]
    fn test_vote_wrong_reveal() {
        let kp = Keypair::generate();
        let solution_id = Hash::ZERO;

        let vote = VerificationVote::commit(
            solution_id,
            *kp.public_key(),
            VoteResult::Accept,
            85,
        );

        let mut public = vote.public_commitment();

        // Wrong vote value should fail
        assert!(public.reveal(
            VoteResult::Reject, // Wrong!
            85,
            vote.nonce.expect("should have nonce"),
        ).is_err());
    }

    #[test]
    fn test_voting_results() {
        let solution_id = Hash::ZERO;

        let votes: Vec<VerificationVote> = (0..5).map(|i| {
            let kp = Keypair::generate();
            VerificationVote::commit(
                solution_id,
                *kp.public_key(),
                if i < 3 { VoteResult::Accept } else { VoteResult::Reject },
                80,
            )
        }).collect();

        let results = VotingResults::from_votes(&votes);

        assert_eq!(results.total_votes, 5);
        assert_eq!(results.accept_votes, 3);
        assert_eq!(results.reject_votes, 2);
        assert_eq!(results.majority, Some(VoteResult::Accept));
        assert_eq!(results.accept_percentage(), 60.0);
    }
}
