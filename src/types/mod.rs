//! Core data types for the `HardClaw` protocol.

mod address;
mod amount;
mod job;
mod solution;
mod block;
mod verification;

pub use address::Address;
pub use amount::HclawAmount;
pub use job::{JobPacket, JobType, JobStatus, VerificationSpec};
pub use solution::{SolutionCandidate, SolutionStatus};
pub use block::{Block, BlockHeader, VerifierAttestation};
pub use verification::{VerificationResult, VerificationVote, VoteResult, VotingResults};

use chrono::{DateTime, Utc};

/// A unique identifier (typically a hash)
pub type Id = crate::crypto::Hash;

/// Unix timestamp in milliseconds
pub type Timestamp = i64;

/// Get current timestamp in milliseconds
#[must_use]
pub fn now_millis() -> Timestamp {
    Utc::now().timestamp_millis()
}

/// Convert timestamp to DateTime
#[must_use]
pub fn timestamp_to_datetime(ts: Timestamp) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ts)
}
