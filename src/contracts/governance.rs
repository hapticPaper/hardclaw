//! Governance Contract - enables on-chain governance for HardClaw.
//!
//! This contract handles:
//! - Proposal creation and voting
//! - Parameter updates via governance
//! - Contract upgrades
//! - Treasury spending
//! - Emergency pauses

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::contracts::{Contract, ContractError, ContractResult, ContractEvent, ExecutionResult};
use crate::contracts::state::ContractState;
use crate::contracts::transaction::ContractTransaction;
use crate::crypto::Hash;
use crate::types::{Address, GovernanceAction};

/// Governance contract ID (deterministic hash of contract name)
pub const GOVERNANCE_CONTRACT_ID: Hash = Hash::from_bytes([
    0x67, 0x0f, 0x8a, 0x3d, 0x1c, 0xe2, 0x49, 0xb7,
    0x95, 0x2a, 0xf4, 0x68, 0xd0, 0x1b, 0x7c, 0x8e,
    0x52, 0xc9, 0x31, 0xa6, 0x0d, 0xf3, 0x6b, 0x94,
    0x28, 0xe5, 0xb1, 0x47, 0x9d, 0x0c, 0x5f, 0x82,
]);

/// Minimum voting period (7 days in milliseconds)
pub const MIN_VOTING_PERIOD: i64 = 7 * 24 * 60 * 60 * 1000;

/// Quorum requirement (30% of total voting power)
pub const QUORUM_PERCENT: u8 = 30;

/// Approval threshold (66% of votes cast)
pub const APPROVAL_THRESHOLD_PERCENT: u8 = 66;

/// Actions the governance contract can perform
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GovernanceTransactionKind {
    /// Create a new proposal
    CreateProposal {
        /// Title of the proposal
        title: String,
        /// Description
        description: String,
        /// Actions to execute if approved
        actions: Vec<GovernanceAction>,
        /// Voting period end time
        voting_ends_at: i64,
    },
    /// Cast a vote on a proposal
    Vote {
        /// Proposal ID
        proposal_id: Hash,
        /// Vote (true = yes, false = no)
        in_favor: bool,
        /// Voting power (based on stake)
        voting_power: u128,
    },
    /// Execute an approved proposal
    Execute {
        /// Proposal ID
        proposal_id: Hash,
    },
}

/// Proposal status
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    /// Currently accepting votes
    Active,
    /// Voting period ended, awaiting execution
    Passed,
    /// Voting period ended, did not pass
    Rejected,
    /// Successfully executed
    Executed,
    /// Failed to execute
    ExecutionFailed,
}

/// A governance proposal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    /// Proposal ID
    pub id: Hash,
    /// Creator
    pub proposer: Address,
    /// Title
    pub title: String,
    /// Description
    pub description: String,
    /// Actions to execute
    pub actions: Vec<GovernanceAction>,
    /// Creation timestamp
    pub created_at: i64,
    /// Voting ends at
    pub voting_ends_at: i64,
    /// Votes in favor (voting power)
    pub yes_votes: u128,
    /// Votes against (voting power)
    pub no_votes: u128,
    /// Voters (to prevent double voting)
    pub voters: HashMap<Address, bool>,
    /// Current status
    pub status: ProposalStatus,
}

/// Governance contract
pub struct GovernanceContract {
    /// Contract ID
    id: Hash,
    /// Proposals by ID
    proposals: HashMap<Hash, Proposal>,
    /// Total voting power (sum of all stakes)
    total_voting_power: u128,
}

impl GovernanceContract {
    /// Create new governance contract
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: GOVERNANCE_CONTRACT_ID,
            proposals: HashMap::new(),
            total_voting_power: 0,
        }
    }

    /// Parse action from transaction input
    fn parse_action(input: &[u8]) -> ContractResult<GovernanceTransactionKind> {
        bincode::deserialize(input).map_err(|e| {
            ContractError::InvalidTransaction(format!("Failed to parse action: {}", e))
        })
    }

    /// Create a new proposal
    fn execute_create_proposal(
        &mut self,
        state: &mut ContractState<'_>,
        proposer: Address,
        title: String,
        description: String,
        actions: Vec<GovernanceAction>,
        voting_ends_at: i64,
    ) -> ContractResult<Hash> {
        // Validate voting period
        let now = crate::types::now_millis();
        if voting_ends_at <= now {
            return Err(ContractError::ExecutionFailed(
                "Voting end time must be in the future".to_string(),
            ));
        }

        if voting_ends_at - now < MIN_VOTING_PERIOD {
            return Err(ContractError::ExecutionFailed(format!(
                "Voting period must be at least {} days",
                MIN_VOTING_PERIOD / (24 * 60 * 60 * 1000)
            )));
        }

        // Generate proposal ID
        let proposal_id = crate::crypto::hash_data(&bincode::serialize(&(
            &proposer,
            &title,
            &description,
            &actions,
            now,
        )).unwrap());

        // Create proposal
        let proposal = Proposal {
            id: proposal_id,
            proposer,
            title: title.clone(),
            description: description.clone(),
            actions,
            created_at: now,
            voting_ends_at,
            yes_votes: 0,
            no_votes: 0,
            voters: HashMap::new(),
            status: ProposalStatus::Active,
        };

        // Store proposal
        let proposal_key = format!("proposal:{}", hex::encode(proposal_id.as_bytes()));
        let proposal_data = bincode::serialize(&proposal)
            .map_err(|e| ContractError::ExecutionFailed(format!("Serialization failed: {}", e)))?;
        
        state.storage_write(proposer, proposal_key.as_bytes().to_vec(), proposal_data);

        self.proposals.insert(proposal_id, proposal);

        // Emit event
        state.emit_event(ContractEvent {
            contract_id: self.id,
            topic: "ProposalCreated".to_string(),
            data: bincode::serialize(&(proposal_id, title)).unwrap(),
        });

        Ok(proposal_id)
    }

    /// Cast a vote on a proposal
    fn execute_vote(
        &mut self,
        state: &mut ContractState<'_>,
        voter: Address,
        proposal_id: Hash,
        in_favor: bool,
        voting_power: u128,
    ) -> ContractResult<()> {
        // Get proposal
        let proposal = self.proposals.get_mut(&proposal_id)
            .ok_or_else(|| ContractError::ExecutionFailed("Proposal not found".to_string()))?;

        // Check proposal is active
        if proposal.status != ProposalStatus::Active {
            return Err(ContractError::ExecutionFailed(
                "Proposal is not active".to_string(),
            ));
        }

        // Check voting period not ended
        let now = crate::types::now_millis();
        if now >= proposal.voting_ends_at {
            return Err(ContractError::ExecutionFailed(
                "Voting period has ended".to_string(),
            ));
        }

        // Check not already voted
        if proposal.voters.contains_key(&voter) {
            return Err(ContractError::ExecutionFailed(
                "Already voted on this proposal".to_string(),
            ));
        }

        // Record vote
        proposal.voters.insert(voter, in_favor);
        if in_favor {
            proposal.yes_votes += voting_power;
        } else {
            proposal.no_votes += voting_power;
        }

        // Emit event
        state.emit_event(ContractEvent {
            contract_id: self.id,
            topic: "VoteCast".to_string(),
            data: bincode::serialize(&(proposal_id, voter, in_favor, voting_power)).unwrap(),
        });

        Ok(())
    }

    /// Execute an approved proposal
    fn execute_proposal(
        &mut self,
        state: &mut ContractState<'_>,
        proposal_id: Hash,
    ) -> ContractResult<()> {
        // Get proposal
        let proposal = self.proposals.get_mut(&proposal_id)
            .ok_or_else(|| ContractError::ExecutionFailed("Proposal not found".to_string()))?;

        // Check voting period ended
        let now = crate::types::now_millis();
        if now < proposal.voting_ends_at {
            return Err(ContractError::ExecutionFailed(
                "Voting period not yet ended".to_string(),
            ));
        }

        // Calculate quorum
        let total_votes = proposal.yes_votes + proposal.no_votes;
        let quorum = self.total_voting_power * u128::from(QUORUM_PERCENT) / 100;
        
        if total_votes < quorum {
            proposal.status = ProposalStatus::Rejected;
            return Err(ContractError::ExecutionFailed(
                "Quorum not reached".to_string(),
            ));
        }

        // Check approval threshold
        let approval_percent = if total_votes > 0 {
            proposal.yes_votes * 100 / total_votes
        } else {
            0
        };

        if approval_percent < u128::from(APPROVAL_THRESHOLD_PERCENT) {
            proposal.status = ProposalStatus::Rejected;
            return Err(ContractError::ExecutionFailed(
                "Approval threshold not met".to_string(),
            ));
        }

        // Mark as passed
        proposal.status = ProposalStatus::Passed;

        // Clone actions to avoid borrow checker issues
        let actions_to_execute = proposal.actions.clone();

        // Execute actions
        for action in &actions_to_execute {
            match self.execute_governance_action(state, action) {
                Ok(()) => {}
                Err(e) => {
                    // Re-get proposal to update status
                    if let Some(p) = self.proposals.get_mut(&proposal_id) {
                        p.status = ProposalStatus::ExecutionFailed;
                    }
                    return Err(e);
                }
            }
        }

        // Mark as executed
        if let Some(p) = self.proposals.get_mut(&proposal_id) {
            p.status = ProposalStatus::Executed;
        }

        // Emit event
        state.emit_event(ContractEvent {
            contract_id: self.id,
            topic: "ProposalExecuted".to_string(),
            data: bincode::serialize(&proposal_id).unwrap(),
        });

        Ok(())
    }

    /// Execute a governance action
    fn execute_governance_action(
        &mut self,
        state: &mut ContractState<'_>,
        action: &GovernanceAction,
    ) -> ContractResult<()> {
        match action {
            GovernanceAction::ParameterUpdate { key, value } => {
                // Update chain parameter
                let param_key = format!("param:{}", key);
                state.storage_write(
                    Address::from_bytes([0; 20]), // System address
                    param_key.as_bytes().to_vec(),
                    value.clone(),
                );
                Ok(())
            }
            GovernanceAction::ContractUpgrade { contract_id, new_code, new_code_hash } => {
                // Store new contract code
                let upgrade_key = format!("upgrade:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    upgrade_key.as_bytes().to_vec(),
                    new_code.clone(),
                );
                // Store code hash for verification
                let hash_key = format!("upgrade_hash:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    hash_key.as_bytes().to_vec(),
                    new_code_hash.as_bytes().to_vec(),
                );
                Ok(())
            }
            GovernanceAction::TreasurySpend { recipient, amount, purpose } => {
                // Transfer from treasury (system address) to recipient
                let treasury = Address::from_bytes([0; 20]);
                state.transfer(treasury, *recipient, *amount)?;
                
                // Log the purpose
                let spend_key = format!("treasury_spend:{}", hex::encode(recipient.as_bytes()));
                state.storage_write(
                    treasury,
                    spend_key.as_bytes().to_vec(),
                    purpose.as_bytes().to_vec(),
                );
                Ok(())
            }
            GovernanceAction::EmergencyPause { contract_id, reason } => {
                // Mark contract as paused
                let pause_key = format!("paused:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    pause_key.as_bytes().to_vec(),
                    reason.as_bytes().to_vec(),
                );
                Ok(())
            }
            GovernanceAction::Resume { contract_id } => {
                // Remove pause marker
                let pause_key = format!("paused:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    pause_key.as_bytes().to_vec(),
                    Vec::new(), // Empty value = not paused
                );
                Ok(())
            }
        }
    }

    /// Update total voting power (called when stakes change)
    pub fn update_voting_power(&mut self, total_power: u128) {
        self.total_voting_power = total_power;
    }
}

impl Default for GovernanceContract {
    fn default() -> Self {
        Self::new()
    }
}

impl Contract for GovernanceContract {
    fn id(&self) -> Hash {
        self.id
    }

    fn name(&self) -> &str {
        "GovernanceContract"
    }

    fn version(&self) -> u32 {
        1
    }

    fn execute(
        &self,
        state: &mut ContractState<'_>,
        tx: &ContractTransaction,
    ) -> ContractResult<ExecutionResult> {
        // Parse action
        let action = Self::parse_action(&tx.input)?;

        // Clone self to make it mutable for execution
        let mut contract = self.clone();

        // Execute based on action
        match action {
            GovernanceTransactionKind::CreateProposal {
                title,
                description,
                actions,
                voting_ends_at,
            } => {
                contract.execute_create_proposal(
                    state,
                    tx.sender_address,
                    title,
                    description,
                    actions,
                    voting_ends_at,
                )?;
            }
            GovernanceTransactionKind::Vote {
                proposal_id,
                in_favor,
                voting_power,
            } => {
                contract.execute_vote(state, tx.sender_address, proposal_id, in_favor, voting_power)?;
            }
            GovernanceTransactionKind::Execute { proposal_id } => {
                contract.execute_proposal(state, proposal_id)?;
            }
        }

        // Return execution result
        Ok(ExecutionResult {
            new_state_root: state.compute_state_root(),
            gas_used: 150_000, // Governance operations are more expensive
            events: state.events().to_vec(),
            output: Vec::new(),
        })
    }

    fn verify(
        &self,
        state: &ContractState<'_>,
        _tx: &ContractTransaction,
        result: &ExecutionResult,
    ) -> ContractResult<bool> {
        // Verify state root matches
        let computed_root = state.compute_state_root();
        Ok(computed_root == result.new_state_root)
    }

    fn is_upgradeable(&self) -> bool {
        true // Governance contract can be upgraded via governance
    }
}

impl Clone for GovernanceContract {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            proposals: self.proposals.clone(),
            total_voting_power: self.total_voting_power,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::state::ContractState;
    use crate::crypto::Keypair;
    use crate::types::{Address, HclawAmount};
    use std::collections::HashMap;

    #[test]
    fn test_create_proposal() {
        let mut contract = GovernanceContract::new();
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let proposer = Address::from_public_key(kp.public_key());

        accounts.insert(
            proposer,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        let now = crate::types::now_millis();
        let voting_ends = now + MIN_VOTING_PERIOD + 1000;

        let proposal_id = contract.execute_create_proposal(
            &mut state,
            proposer,
            "Test Proposal".to_string(),
            "This is a test".to_string(),
            vec![],
            voting_ends,
        );

        assert!(proposal_id.is_ok());
        assert_eq!(contract.proposals.len(), 1);
    }

    #[test]
    fn test_vote_on_proposal() {
        let mut contract = GovernanceContract::new();
        contract.update_voting_power(10000);

        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let voter = Address::from_public_key(kp.public_key());

        accounts.insert(
            voter,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        // Create proposal
        let now = crate::types::now_millis();
        let voting_ends = now + MIN_VOTING_PERIOD + 1000;
        let proposal_id = contract.execute_create_proposal(
            &mut state,
            voter,
            "Test".to_string(),
            "Test".to_string(),
            vec![],
            voting_ends,
        ).unwrap();

        // Vote
        let result = contract.execute_vote(&mut state, voter, proposal_id, true, 100);
        assert!(result.is_ok());

        let proposal = &contract.proposals[&proposal_id];
        assert_eq!(proposal.yes_votes, 100);
    }

    #[test]
    fn test_cannot_vote_twice() {
        let mut contract = GovernanceContract::new();
        contract.update_voting_power(10000);

        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let voter = Address::from_public_key(kp.public_key());

        accounts.insert(
            voter,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        let now = crate::types::now_millis();
        let voting_ends = now + MIN_VOTING_PERIOD + 1000;
        let proposal_id = contract.execute_create_proposal(
            &mut state,
            voter,
            "Test".to_string(),
            "Test".to_string(),
            vec![],
            voting_ends,
        ).unwrap();

        // First vote succeeds
        assert!(contract.execute_vote(&mut state, voter, proposal_id, true, 100).is_ok());

        // Second vote fails
        assert!(contract.execute_vote(&mut state, voter, proposal_id, true, 100).is_err());
    }
}
