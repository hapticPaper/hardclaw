//! Governance Contract - enables on-chain governance for HardClaw.
//!
//! This contract handles:
//! - Proposal creation and voting
//! - Parameter updates via governance
//! - Contract upgrades
//! - Treasury spending
//! - Emergency pauses
//!
//! All state is storage-backed — the contract struct is stateless.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::contracts::state::ContractState;
use crate::contracts::transaction::ContractTransaction;
use crate::contracts::{Contract, ContractError, ContractEvent, ContractResult, ExecutionResult};
use crate::crypto::Hash;
use crate::types::{Address, GovernanceAction};

/// Governance contract ID (deterministic hash of contract name)
pub const GOVERNANCE_CONTRACT_ID: Hash = Hash::from_bytes([
    0x67, 0x0f, 0x8a, 0x3d, 0x1c, 0xe2, 0x49, 0xb7, 0x95, 0x2a, 0xf4, 0x68, 0xd0, 0x1b, 0x7c, 0x8e,
    0x52, 0xc9, 0x31, 0xa6, 0x0d, 0xf3, 0x6b, 0x94, 0x28, 0xe5, 0xb1, 0x47, 0x9d, 0x0c, 0x5f, 0x82,
]);

/// Minimum voting period (7 days in milliseconds)
pub const MIN_VOTING_PERIOD: i64 = 7 * 24 * 60 * 60 * 1000;

/// Quorum requirement (30% of total voting power)
pub const QUORUM_PERCENT: u8 = 30;

/// Approval threshold (66% of votes cast)
pub const APPROVAL_THRESHOLD_PERCENT: u8 = 66;

// Storage keys
const KEY_TOTAL_VOTING_POWER: &[u8] = b"gov:total_voting_power";
const KEY_PROPOSAL_INDEX: &[u8] = b"gov:proposal_index";
const PROPOSAL_PREFIX: &[u8] = b"gov:proposal:";

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
    /// Update total voting power (called when stakes change)
    UpdateVotingPower {
        /// New total voting power
        total_power: u128,
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

/// Governance contract — fully storage-backed, no in-memory state.
#[derive(Clone)]
pub struct GovernanceContract {
    /// Contract ID
    id: Hash,
}

impl GovernanceContract {
    /// Create new governance contract
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: GOVERNANCE_CONTRACT_ID,
        }
    }

    /// The contract address used for storage keys
    fn contract_address(&self) -> Address {
        // Derive a stable address from the contract ID
        let bytes = self.id.as_bytes();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&bytes[..20]);
        Address::from_bytes(addr)
    }

    // --- Storage helpers ---

    fn proposal_key(proposal_id: &Hash) -> Vec<u8> {
        let mut key = PROPOSAL_PREFIX.to_vec();
        key.extend_from_slice(proposal_id.as_bytes());
        key
    }

    fn load_proposal(&self, state: &ContractState<'_>, proposal_id: &Hash) -> Option<Proposal> {
        let key = Self::proposal_key(proposal_id);
        let data = state.storage_read(&self.contract_address(), &key)?;
        bincode::deserialize(&data).ok()
    }

    fn save_proposal(&self, state: &mut ContractState<'_>, proposal: &Proposal) {
        let key = Self::proposal_key(&proposal.id);
        let data = bincode::serialize(proposal).expect("proposal serialization");
        state.storage_write(self.contract_address(), key, data);
    }

    fn load_total_voting_power(&self, state: &ContractState<'_>) -> u128 {
        state
            .storage_read(&self.contract_address(), KEY_TOTAL_VOTING_POWER)
            .and_then(|d| bincode::deserialize(&d).ok())
            .unwrap_or(0)
    }

    fn save_total_voting_power(&self, state: &mut ContractState<'_>, power: u128) {
        let data = bincode::serialize(&power).expect("u128 serialization");
        state.storage_write(
            self.contract_address(),
            KEY_TOTAL_VOTING_POWER.to_vec(),
            data,
        );
    }

    fn load_proposal_index(&self, state: &ContractState<'_>) -> Vec<Hash> {
        state
            .storage_read(&self.contract_address(), KEY_PROPOSAL_INDEX)
            .and_then(|d| bincode::deserialize(&d).ok())
            .unwrap_or_default()
    }

    fn save_proposal_index(&self, state: &mut ContractState<'_>, index: &[Hash]) {
        let data = bincode::serialize(index).expect("proposal index serialization");
        state.storage_write(self.contract_address(), KEY_PROPOSAL_INDEX.to_vec(), data);
    }

    /// Parse action from transaction input
    fn parse_action(input: &[u8]) -> ContractResult<GovernanceTransactionKind> {
        bincode::deserialize(input).map_err(|e| {
            ContractError::InvalidTransaction(format!("Failed to parse action: {}", e))
        })
    }

    /// Create a new proposal
    fn execute_create_proposal(
        &self,
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
        let proposal_id = crate::crypto::hash_data(
            &bincode::serialize(&(&proposer, &title, &description, &actions, now)).unwrap(),
        );

        // Create proposal
        let proposal = Proposal {
            id: proposal_id,
            proposer,
            title: title.clone(),
            description,
            actions,
            created_at: now,
            voting_ends_at,
            yes_votes: 0,
            no_votes: 0,
            voters: HashMap::new(),
            status: ProposalStatus::Active,
        };

        // Save to storage
        self.save_proposal(state, &proposal);

        // Update proposal index
        let mut index = self.load_proposal_index(state);
        index.push(proposal_id);
        self.save_proposal_index(state, &index);

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
        &self,
        state: &mut ContractState<'_>,
        voter: Address,
        proposal_id: Hash,
        in_favor: bool,
        voting_power: u128,
    ) -> ContractResult<()> {
        // Load proposal from storage
        let mut proposal = self
            .load_proposal(state, &proposal_id)
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

        // Save updated proposal back to storage
        self.save_proposal(state, &proposal);

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
        &self,
        state: &mut ContractState<'_>,
        proposal_id: Hash,
    ) -> ContractResult<()> {
        // Load proposal from storage
        let mut proposal = self
            .load_proposal(state, &proposal_id)
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
        let total_voting_power = self.load_total_voting_power(state);
        let quorum = total_voting_power * u128::from(QUORUM_PERCENT) / 100;

        if total_votes < quorum {
            proposal.status = ProposalStatus::Rejected;
            self.save_proposal(state, &proposal);
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
            self.save_proposal(state, &proposal);
            return Err(ContractError::ExecutionFailed(
                "Approval threshold not met".to_string(),
            ));
        }

        // Mark as passed
        proposal.status = ProposalStatus::Passed;

        // Clone actions to execute
        let actions_to_execute = proposal.actions.clone();

        // Execute actions
        for action in &actions_to_execute {
            if let Err(e) = self.execute_governance_action(state, action) {
                proposal.status = ProposalStatus::ExecutionFailed;
                self.save_proposal(state, &proposal);
                return Err(e);
            }
        }

        // Mark as executed
        proposal.status = ProposalStatus::Executed;
        self.save_proposal(state, &proposal);

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
        &self,
        state: &mut ContractState<'_>,
        action: &GovernanceAction,
    ) -> ContractResult<()> {
        match action {
            GovernanceAction::ParameterUpdate { key, value } => {
                let param_key = format!("param:{}", key);
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    param_key.as_bytes().to_vec(),
                    value.clone(),
                );
                Ok(())
            }
            GovernanceAction::ContractUpgrade {
                contract_id,
                new_code,
                new_code_hash,
            } => {
                let upgrade_key = format!("upgrade:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    upgrade_key.as_bytes().to_vec(),
                    new_code.clone(),
                );
                let hash_key = format!("upgrade_hash:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    hash_key.as_bytes().to_vec(),
                    new_code_hash.as_bytes().to_vec(),
                );
                Ok(())
            }
            GovernanceAction::TreasurySpend {
                recipient,
                amount,
                purpose,
            } => {
                let treasury = Address::from_bytes([0; 20]);
                state.transfer(treasury, *recipient, *amount)?;

                let spend_key = format!("treasury_spend:{}", hex::encode(recipient.as_bytes()));
                state.storage_write(
                    treasury,
                    spend_key.as_bytes().to_vec(),
                    purpose.as_bytes().to_vec(),
                );
                Ok(())
            }
            GovernanceAction::EmergencyPause {
                contract_id,
                reason,
            } => {
                let pause_key = format!("paused:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    pause_key.as_bytes().to_vec(),
                    reason.as_bytes().to_vec(),
                );
                Ok(())
            }
            GovernanceAction::Resume { contract_id } => {
                let pause_key = format!("paused:{}", hex::encode(contract_id.as_bytes()));
                state.storage_write(
                    Address::from_bytes([0; 20]),
                    pause_key.as_bytes().to_vec(),
                    Vec::new(),
                );
                Ok(())
            }
        }
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
        let action = Self::parse_action(&tx.input)?;

        match action {
            GovernanceTransactionKind::CreateProposal {
                title,
                description,
                actions,
                voting_ends_at,
            } => {
                self.execute_create_proposal(
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
                self.execute_vote(
                    state,
                    tx.sender_address,
                    proposal_id,
                    in_favor,
                    voting_power,
                )?;
            }
            GovernanceTransactionKind::Execute { proposal_id } => {
                self.execute_proposal(state, proposal_id)?;
            }
            GovernanceTransactionKind::UpdateVotingPower { total_power } => {
                self.save_total_voting_power(state, total_power);
            }
        }

        Ok(ExecutionResult {
            new_state_root: state.compute_state_root(),
            gas_used: 150_000,
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
        let computed_root = state.compute_state_root();
        Ok(computed_root == result.new_state_root)
    }

    fn is_upgradeable(&self) -> bool {
        true
    }

    fn on_deploy(&self, state: &mut ContractState<'_>, _init_data: &[u8]) -> ContractResult<()> {
        // Initialize storage with defaults
        self.save_total_voting_power(state, 0);
        self.save_proposal_index(state, &[]);
        Ok(())
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
        let contract = GovernanceContract::new();
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

        // Verify proposal is in storage
        let pid = proposal_id.unwrap();
        let loaded = contract.load_proposal(&state, &pid);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().title, "Test Proposal");

        // Verify proposal index
        let index = contract.load_proposal_index(&state);
        assert_eq!(index.len(), 1);
        assert_eq!(index[0], pid);
    }

    #[test]
    fn test_vote_on_proposal() {
        let contract = GovernanceContract::new();

        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let voter = Address::from_public_key(kp.public_key());

        accounts.insert(
            voter,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        // Set voting power in storage
        contract.save_total_voting_power(&mut state, 10000);

        // Create proposal
        let now = crate::types::now_millis();
        let voting_ends = now + MIN_VOTING_PERIOD + 1000;
        let proposal_id = contract
            .execute_create_proposal(
                &mut state,
                voter,
                "Test".to_string(),
                "Test".to_string(),
                vec![],
                voting_ends,
            )
            .unwrap();

        // Vote
        let result = contract.execute_vote(&mut state, voter, proposal_id, true, 100);
        assert!(result.is_ok());

        // Verify vote persisted in storage
        let proposal = contract.load_proposal(&state, &proposal_id).unwrap();
        assert_eq!(proposal.yes_votes, 100);
        assert!(proposal.voters.contains_key(&voter));
    }

    #[test]
    fn test_cannot_vote_twice() {
        let contract = GovernanceContract::new();

        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let voter = Address::from_public_key(kp.public_key());

        accounts.insert(
            voter,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        contract.save_total_voting_power(&mut state, 10000);

        let now = crate::types::now_millis();
        let voting_ends = now + MIN_VOTING_PERIOD + 1000;
        let proposal_id = contract
            .execute_create_proposal(
                &mut state,
                voter,
                "Test".to_string(),
                "Test".to_string(),
                vec![],
                voting_ends,
            )
            .unwrap();

        // First vote succeeds
        assert!(contract
            .execute_vote(&mut state, voter, proposal_id, true, 100)
            .is_ok());

        // Second vote fails
        assert!(contract
            .execute_vote(&mut state, voter, proposal_id, true, 100)
            .is_err());
    }

    #[test]
    fn test_state_persists_across_calls() {
        let contract = GovernanceContract::new();

        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp1 = Keypair::generate();
        let kp2 = Keypair::generate();
        let voter1 = Address::from_public_key(kp1.public_key());
        let voter2 = Address::from_public_key(kp2.public_key());

        accounts.insert(
            voter1,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );
        accounts.insert(
            voter2,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        // Scope 1: create proposal
        let proposal_id = {
            let mut state = ContractState::new(&mut accounts, &mut storage);
            contract.save_total_voting_power(&mut state, 10000);

            let now = crate::types::now_millis();
            let voting_ends = now + MIN_VOTING_PERIOD + 1000;
            let pid = contract
                .execute_create_proposal(
                    &mut state,
                    voter1,
                    "Persist Test".to_string(),
                    "Test".to_string(),
                    vec![],
                    voting_ends,
                )
                .unwrap();
            state.commit();
            pid
        };

        // Scope 2: vote with voter1 (fresh state wrapper)
        {
            let mut state = ContractState::new(&mut accounts, &mut storage);
            contract
                .execute_vote(&mut state, voter1, proposal_id, true, 500)
                .unwrap();
            state.commit();
        }

        // Scope 3: vote with voter2 (fresh state wrapper)
        {
            let mut state = ContractState::new(&mut accounts, &mut storage);
            contract
                .execute_vote(&mut state, voter2, proposal_id, false, 300)
                .unwrap();
            state.commit();
        }

        // Verify: both votes visible in storage
        let state = ContractState::new(&mut accounts, &mut storage);
        let proposal = contract.load_proposal(&state, &proposal_id).unwrap();
        assert_eq!(proposal.yes_votes, 500);
        assert_eq!(proposal.no_votes, 300);
        assert_eq!(proposal.voters.len(), 2);
    }

    #[test]
    fn test_on_deploy_initializes_storage() {
        let contract = GovernanceContract::new();
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();
        let mut state = ContractState::new(&mut accounts, &mut storage);

        contract.on_deploy(&mut state, &[]).unwrap();

        assert_eq!(contract.load_total_voting_power(&state), 0);
        assert!(contract.load_proposal_index(&state).is_empty());
    }
}
