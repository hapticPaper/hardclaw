//! Genesis contract initialization.
//!
//! This module handles the deployment and registration of genesis contracts
//! in the genesis block.

use crate::contracts::genesis_bounty::GenesisBountyContract;
use crate::contracts::governance::GovernanceContract;
use crate::contracts::processor::TransactionProcessor;
use crate::contracts::ContractRegistry;

/// Initialize genesis contracts and return a configured transaction processor.
///
/// This creates and registers:
/// - `GenesisBountyContract`: Handles participant joining and bounty distribution
/// - `GovernanceContract`: Enables on-chain governance from day 1
///
/// # Arguments
/// * `bounty_start_time` - Timestamp when bounty period starts (typically genesis timestamp)
/// * `_initial_voting_power` - Total voting power (set via `UpdateVotingPower` transaction after deploy)
///
/// # Returns
/// A `TransactionProcessor` with both contracts registered and ready for execution
pub fn initialize_genesis_contracts(
    bounty_start_time: i64,
    _initial_voting_power: u128,
) -> TransactionProcessor {
    let mut registry = ContractRegistry::new();

    // Create and register genesis bounty contract
    let bounty_contract = GenesisBountyContract::new(bounty_start_time);
    registry.register(Box::new(bounty_contract));

    // Create and register governance contract
    // Voting power is storage-backed and initialized to 0 via on_deploy.
    // Use an UpdateVotingPower transaction to set initial voting power.
    let governance_contract = GovernanceContract::new();
    registry.register(Box::new(governance_contract));

    // Create transaction processor with initialized registry
    // Use default max gas (10M)
    TransactionProcessor::with_registry(10_000_000, registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_genesis_contracts() {
        let start_time = 1_000_000_i64;
        let voting_power = 1_000_000_u128;

        let processor = initialize_genesis_contracts(start_time, voting_power);

        // Verify both contracts were registered
        assert_eq!(processor.registry().contract_count(), 2);
    }

    #[test]
    fn test_contracts_have_correct_ids() {
        let start_time = 1_000_000_i64;
        let processor = initialize_genesis_contracts(start_time, 100_000);

        // Verify we can look up contracts by their IDs
        let bounty_id = crate::contracts::genesis_bounty::GENESIS_BOUNTY_CONTRACT_ID;
        let governance_id = crate::contracts::governance::GOVERNANCE_CONTRACT_ID;

        assert!(processor.registry().get_contract(&bounty_id).is_some());
        assert!(processor.registry().get_contract(&governance_id).is_some());
    }
}
