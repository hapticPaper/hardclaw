//! # HardClaw Protocol
//!
//! A Proof-of-Verification Protocol for the Autonomous Agent Economy.
//!
//! ## Architecture
//!
//! The protocol consists of three actor roles:
//! - **Requester**: Submits Job Packets (Inputs + Bounty)
//! - **Solver**: Executes NP-Hard work, submits Solution Candidates
//! - **Verifier**: Mines blocks by verifying solutions
//!
//! ## Security Model
//!
//! - Honey Pot injection defends against lazy miners
//! - Burn-to-Request prevents Sybil attacks
//! - 66% consensus threshold for block validity
//! - Schelling Point consensus for subjective tasks

#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    rust_2018_idioms
)]
#![warn(missing_docs)]
#![allow(clippy::module_name_repetitions)]

pub mod crypto;
pub mod types;
pub mod consensus;
pub mod verifier;
pub mod schelling;
pub mod tokenomics;
pub mod mempool;
pub mod state;
pub mod network;
pub mod wallet;

pub use types::{
    Address, JobPacket, SolutionCandidate, Block, BlockHeader,
    JobType, VerificationResult, HclawAmount,
};
pub use crypto::{Keypair, PublicKey, Signature, Hash};
pub use consensus::ProofOfVerification;
pub use verifier::Verifier;
pub use tokenomics::TokenEconomics;
pub use wallet::{Wallet, WalletInfo, WalletError};
pub use network::{NetworkNode, NetworkConfig, NetworkEvent, NetworkError, NetworkMessage, PeerInfo};

/// Protocol version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Consensus threshold (66% = 2/3 majority)
pub const CONSENSUS_THRESHOLD: f64 = 0.66;

/// Schelling redundancy (jobs sent to N solvers for subjective tasks)
pub const SCHELLING_REDUNDANCY: usize = 5;

/// Fee distribution constants
pub mod fees {
    /// Percentage to solver (worker)
    pub const SOLVER_SHARE: u8 = 95;
    /// Percentage to verifier (miner)
    pub const VERIFIER_SHARE: u8 = 4;
    /// Percentage burned
    pub const BURN_SHARE: u8 = 1;
}
