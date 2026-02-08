//! Governance action types for on-chain proposals

use serde::{Deserialize, Serialize};

use crate::crypto::Hash;
use crate::types::{Address, HclawAmount, Id};

/// Types of governance actions that can be proposed
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernanceAction {
    /// Update a chain parameter
    ParameterUpdate {
        /// Parameter key (e.g., "min_stake", "block_time_ms")
        key: String,
        /// New value (encoded as bytes)
        value: Vec<u8>,
    },
    /// Upgrade a contract to new code
    ContractUpgrade {
        /// Contract ID to upgrade
        contract_id: Id,
        /// Hash of new contract code
        new_code_hash: Hash,
        /// New code bytecode
        new_code: Vec<u8>,
    },
    /// Spend from treasury/unallocated pools
    TreasurySpend {
        /// Recipient address
        recipient: Address,
        /// Amount to spend
        amount: HclawAmount,
        /// Purpose/justification
        purpose: String,
    },
    /// Emergency pause a contract
    EmergencyPause {
        /// Contract ID to pause
        contract_id: Id,
        /// Reason for pause
        reason: String,
    },
    /// Resume a paused contract
    Resume {
        /// Contract ID to resume
        contract_id: Id,
    },
}
