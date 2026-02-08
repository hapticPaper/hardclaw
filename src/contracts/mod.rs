//! Smart contract execution framework.
//!
//! This module provides the core infrastructure for executing smart contracts
//! on `HardClaw`. Contracts are stateful programs that:
//! - Define verification tasks
//! - Mutate chain state atomically
//! - Can be governed via on-chain voting
//!
//! ## Architecture
//!
//! 1. **Contracts**: Implement the `Contract` trait to define logic
//! 2. **Transactions**: Propose state changes via verified jobs
//! 3. **State Transitions**: Execute contract logic atomically
//! 4. **Registry**: Track deployed contracts and route execution
//!
//! ## Enforcement Mechanism
//!
//! The key enforcement mechanism is the **state transition engine**:
//! - Verifiers execute contract code deterministically
//! - State changes are computed and verified via state root hashes
//! - Invalid transitions are rejected by consensus
//! - All state mutations are atomic (all-or-nothing)

pub mod genesis_bounty;
pub mod governance;
pub mod processor;
pub mod state;
pub mod transaction;

use crate::crypto::Hash;
use crate::types::{HclawAmount, Id};

use self::state::ContractState;
use self::transaction::ContractTransaction;

/// Result type for contract operations
pub type ContractResult<T> = Result<T, ContractError>;

/// A smart contract that can be executed on-chain
pub trait Contract: Send + Sync {
    /// Unique contract ID (hash of code + metadata)
    fn id(&self) -> Id;

    /// Human-readable contract name
    fn name(&self) -> &str;

    /// Contract version
    fn version(&self) -> u32;

    /// Execute a transaction against this contract
    ///
    /// This method receives the current chain state and a transaction.
    /// It should:
    /// 1. Verify the transaction is valid
    /// 2. Compute state mutations
    /// 3. Return the new state or an error
    ///
    /// # Atomicity
    /// Either all state changes succeed or all fail. No partial updates.
    ///
    /// # Errors
    /// Returns error if transaction is invalid or execution fails
    fn execute(
        &self,
        state: &mut ContractState<'_>,
        tx: &ContractTransaction,
    ) -> ContractResult<ExecutionResult>;

    /// Verify a proposed execution result
    ///
    /// Other verifiers call this to confirm the state transition is correct.
    /// Should return true if the execution result is valid.
    fn verify(
        &self,
        state: &ContractState<'_>,
        tx: &ContractTransaction,
        result: &ExecutionResult,
    ) -> ContractResult<bool>;

    /// Check if this contract can be upgraded
    fn is_upgradeable(&self) -> bool {
        false
    }

    /// Hook called when contract is deployed
    fn on_deploy(&self, _state: &mut ContractState<'_>) -> ContractResult<()> {
        Ok(())
    }
}

/// Result of contract execution
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecutionResult {
    /// New state root after execution
    pub new_state_root: Hash,
    /// Gas/compute units consumed
    pub gas_used: u64,
    /// Events emitted during execution
    pub events: Vec<ContractEvent>,
    /// Output data (opaque bytes)
    pub output: Vec<u8>,
}

/// Events emitted by contracts
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContractEvent {
    /// Contract that emitted the event
    pub contract_id: Id,
    /// Event topic (for indexing/filtering)
    pub topic: String,
    /// Event data
    pub data: Vec<u8>,
}

/// Contract execution errors
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    /// Contract not found
    #[error("contract not found: {0}")]
    NotFound(Id),

    /// Transaction validation failed
    #[error("invalid transaction: {0}")]
    InvalidTransaction(String),

    /// Execution failed
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Insufficient balance for transaction
    #[error("insufficient balance: need {need}, have {have}")]
    InsufficientBalance {
        /// Amount needed
        need: HclawAmount,
        /// Amount available
        have: HclawAmount,
    },

    /// State root mismatch
    #[error("state root mismatch: expected {expected}, got {got}")]
    StateRootMismatch {
        /// Expected state root
        expected: Hash,
        /// Actual state root
        got: Hash,
    },

    /// Contract is not upgradeable
    #[error("contract is not upgradeable")]
    NotUpgradeable,

    /// Unauthorized access
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Registry of deployed contracts
///
/// Note: Cannot derive Clone or Debug because it contains trait objects
#[derive(Default)]
pub struct ContractRegistry {
    /// Contracts by ID
    contracts: std::collections::HashMap<Id, Box<dyn Contract>>,
}

impl ContractRegistry {
    /// Create new empty registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    /// Register a contract
    pub fn register(&mut self, contract: Box<dyn Contract>) {
        let id = contract.id();
        self.contracts.insert(id, contract);
    }

    /// Get contract by ID
    #[must_use]
    pub fn get(&self, id: &Id) -> Option<&dyn Contract> {
        self.contracts.get(id).map(std::convert::AsRef::as_ref)
    }

    /// Check if contract exists
    #[must_use]
    pub fn contains(&self, id: &Id) -> bool {
        self.contracts.contains_key(id)
    }

    /// List all contract IDs
    #[must_use]
    pub fn list(&self) -> Vec<Id> {
        self.contracts.keys().copied().collect()
    }

    /// Get number of registered contracts
    #[must_use]
    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }

    /// Get contract by ID (alternative name for get)
    #[must_use]
    pub fn get_contract(&self, id: &Id) -> Option<&dyn Contract> {
        self.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry() {
        let registry = ContractRegistry::new();
        assert_eq!(registry.list().len(), 0);
    }
}
