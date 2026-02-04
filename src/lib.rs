//! # `HardClaw` Protocol
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
#![deny(clippy::all, rust_2018_idioms)]
#![warn(clippy::pedantic, clippy::nursery, missing_docs)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::future_not_send,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    // Intentional numeric casts - blockchain amounts and timing are bounded
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    // Const fn not always beneficial for complex types
    clippy::missing_const_for_fn,
    // Self methods kept for API consistency even if unused
    clippy::unused_self,
    // must_use on every fn is excessive
    clippy::must_use_candidate,
    // Pass by value is fine for small Copy types
    clippy::needless_pass_by_value,
    // Field naming matches domain terminology
    clippy::struct_field_names,
    // Match arms with same body are sometimes clearer separate
    clippy::match_same_arms
)]

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
pub use crypto::{
    Keypair, PublicKey, SecretKey, Signature, Hash,
    generate_mnemonic, parse_mnemonic, keypair_from_mnemonic,
    keypair_from_phrase, mnemonic_to_words, MNEMONIC_WORD_COUNT,
};
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
