//! Genesis Bounty Contract - manages participant onboarding and bounty distribution.
//!
//! This contract handles:
//! - Participant joining with minimum stake
//! - Flat 100 HCLAW airdrop distribution
//! - Parabolic daily bounty payouts
//! - Slot machine block winner selection
//! - Proportional reward distribution

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::contracts::state::ContractState;
use crate::contracts::transaction::ContractTransaction;
use crate::contracts::{Contract, ContractError, ContractResult, ExecutionResult};
use crate::crypto::Hash;
use crate::genesis::bounty::{
    distribute_bounty, is_winner_block, BountyTracker, BOUNTY_DAYS, MIN_PUBLIC_NODES,
};
use crate::types::{Address, HclawAmount};

/// Genesis bounty contract ID (deterministic hash of contract name)
pub const GENESIS_BOUNTY_CONTRACT_ID: Hash = Hash::from_bytes([
    0xb0, 0x47, 0x1c, 0xd8, 0x9f, 0x6e, 0x3a, 0x22, 0x5c, 0x94, 0xe1, 0xf2, 0x68, 0xd9, 0x0b, 0x7a,
    0x31, 0x8e, 0x52, 0xc3, 0x1a, 0x0f, 0x93, 0x6d, 0x78, 0xb2, 0x45, 0xf6, 0x29, 0xc8, 0x11, 0x5e,
]);

/// Minimum stake to join genesis (50 HCLAW)
pub const MIN_STAKE: u64 = 50;

/// Flat airdrop amount per participant (100 HCLAW)
pub const AIRDROP_AMOUNT: u64 = 100;

/// Maximum participants (5000)
pub const MAX_PARTICIPANTS: usize = 5_000;

/// Actions the bounty contract can perform
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BountyAction {
    /// Join genesis with stake
    JoinGenesis {
        /// Amount to stake
        stake: HclawAmount,
    },
    /// Claim daily bounty for a winning block
    ClaimBounty {
        /// Bootstrap day (0-29)
        day: u8,
        /// Block hash used for randomness
        block_hash: Hash,
        /// List of contributing validators and their attestation counts
        contributors: Vec<(Address, u32)>,
    },
    /// Update public node count
    UpdateNodeCount {
        /// Number of public nodes
        count: u32,
    },
}

/// Participant state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participant {
    /// Address
    pub address: Address,
    /// Stake amount
    pub stake: HclawAmount,
    /// Airdrop received
    pub airdrop: HclawAmount,
    /// Total bounties earned
    pub bounties_earned: HclawAmount,
    /// Join timestamp
    pub joined_at: u64,
}

/// Genesis bounty contract
pub struct GenesisBountyContract {
    /// Contract ID
    id: Hash,
    /// Bounty tracker
    bounty_tracker: BountyTracker,
    /// Participants by address
    participants: HashMap<Address, Participant>,
    /// Total participants
    participant_count: usize,
}

impl GenesisBountyContract {
    /// Create new genesis bounty contract
    #[must_use]
    pub fn new(start_time: i64) -> Self {
        Self {
            id: GENESIS_BOUNTY_CONTRACT_ID,
            bounty_tracker: BountyTracker::new(start_time),
            participants: HashMap::new(),
            participant_count: 0,
        }
    }

    /// Parse action from transaction input
    fn parse_action(input: &[u8]) -> ContractResult<BountyAction> {
        bincode::deserialize(input).map_err(|e| {
            ContractError::InvalidTransaction(format!("Failed to parse action: {}", e))
        })
    }

    /// Execute join genesis action
    fn execute_join(
        &mut self,
        contract_state: &mut ContractState<'_>,
        sender: Address,
        stake_amount: HclawAmount,
    ) -> ContractResult<()> {
        // Check not already joined
        if self.participants.contains_key(&sender) {
            return Err(ContractError::ExecutionFailed(
                "Already joined genesis".to_string(),
            ));
        }

        // Check participant limit
        if self.participant_count >= MAX_PARTICIPANTS {
            return Err(ContractError::ExecutionFailed(
                "Maximum participants reached".to_string(),
            ));
        }

        // Validate stake
        let min_stake = HclawAmount::from_hclaw(MIN_STAKE);
        if stake_amount < min_stake {
            return Err(ContractError::ExecutionFailed(format!(
                "Stake {} below minimum {}",
                stake_amount, min_stake
            )));
        }

        // Debit stake from participant
        contract_state.debit(sender, stake_amount)?;

        // Credit airdrop to participant
        let airdrop = HclawAmount::from_hclaw(AIRDROP_AMOUNT);
        contract_state.credit(sender, airdrop);

        // Store participant data
        let participant = Participant {
            address: sender,
            stake: stake_amount,
            airdrop,
            bounties_earned: HclawAmount::ZERO,
            joined_at: crate::types::now_millis() as u64,
        };

        let participant_key = format!("participant:{}", hex::encode(sender.as_bytes()));
        let participant_data = bincode::serialize(&participant)
            .map_err(|e| ContractError::ExecutionFailed(format!("Serialization failed: {}", e)))?;

        contract_state.storage_write(
            sender,
            participant_key.as_bytes().to_vec(),
            participant_data,
        );

        // Update participant count
        self.participants.insert(sender, participant);
        self.participant_count += 1;

        // Emit event
        let event_data = bincode::serialize(&(sender, stake_amount)).unwrap();
        contract_state.emit_event(crate::contracts::ContractEvent {
            contract_id: self.id,
            topic: "ParticipantJoined".to_string(),
            data: event_data,
        });

        Ok(())
    }

    /// Execute claim bounty action
    fn execute_claim(
        &mut self,
        state: &mut ContractState<'_>,
        day: u8,
        block_hash: Hash,
        contributors: Vec<(Address, u32)>,
    ) -> ContractResult<()> {
        // Validate day
        if day >= BOUNTY_DAYS {
            return Err(ContractError::ExecutionFailed(format!(
                "Day {} out of range",
                day
            )));
        }

        // Check bounties are active
        if !self.bounty_tracker.is_active() {
            return Err(ContractError::ExecutionFailed(format!(
                "Bounties not active (need {} public nodes, have {})",
                MIN_PUBLIC_NODES, self.bounty_tracker.public_node_count
            )));
        }

        // Check if this block wins
        let threshold = u32::MAX / 10; // 10% win chance
        if !is_winner_block(block_hash, day, threshold) {
            return Err(ContractError::ExecutionFailed(
                "Block is not a winner".to_string(),
            ));
        }

        // Get remaining budget for today
        let remaining = self.bounty_tracker.remaining_today(day);
        if remaining.raw() == 0 {
            return Err(ContractError::ExecutionFailed(
                "No remaining budget for today".to_string(),
            ));
        }

        // Distribute bounty
        let distributions = distribute_bounty(contributors, remaining);

        // Apply distributions
        for (addr, amount) in &distributions {
            state.credit(*addr, *amount);

            // Update participant bounties earned
            if let Some(participant) = self.participants.get_mut(addr) {
                participant.bounties_earned = participant.bounties_earned.saturating_add(*amount);
            }
        }

        // Record payout
        let mut total_distributed = HclawAmount::ZERO;
        for (_, amount) in &distributions {
            total_distributed = total_distributed.saturating_add(*amount);
        }
        self.bounty_tracker.record_payout(day, total_distributed);

        // Emit event
        let event_data = bincode::serialize(&(day, block_hash, total_distributed)).unwrap();
        state.emit_event(crate::contracts::ContractEvent {
            contract_id: self.id,
            topic: "BountyClaimed".to_string(),
            data: event_data,
        });

        Ok(())
    }

    /// Execute update node count action  
    fn execute_update_nodes(
        &mut self,
        _state: &mut ContractState<'_>,
        count: u32,
    ) -> ContractResult<()> {
        self.bounty_tracker.update_node_count(count);
        Ok(())
    }
}

impl Contract for GenesisBountyContract {
    fn id(&self) -> Hash {
        self.id
    }

    fn name(&self) -> &str {
        "GenesisBountyContract"
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
            BountyAction::JoinGenesis { stake } => {
                contract.execute_join(state, tx.sender_address, stake)?;
            }
            BountyAction::ClaimBounty {
                day,
                block_hash,
                contributors,
            } => {
                contract.execute_claim(state, day, block_hash, contributors)?;
            }
            BountyAction::UpdateNodeCount { count } => {
                contract.execute_update_nodes(state, count)?;
            }
        }

        // Return execution result
        Ok(ExecutionResult {
            new_state_root: state.compute_state_root(),
            gas_used: 100_000, // TODO: actual gas metering
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
        false // Genesis contract is immutable
    }
}

impl Clone for GenesisBountyContract {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            bounty_tracker: self.bounty_tracker.clone(),
            participants: self.participants.clone(),
            participant_count: self.participant_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_join_genesis() {
        let mut contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        // Fund sender
        accounts.insert(
            sender,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut contract_state = ContractState::new(&mut accounts, &mut storage);

        // Join with valid stake
        let stake_amount = HclawAmount::from_hclaw(MIN_STAKE);
        let result = contract.execute_join(&mut contract_state, sender, stake_amount);
        assert!(result.is_ok());

        // Verify participant added
        assert_eq!(contract.participant_count, 1);
        assert!(contract.participants.contains_key(&sender));
    }

    #[test]
    fn test_join_fails_below_min_stake() {
        let mut contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        accounts.insert(
            sender,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut contract_state = ContractState::new(&mut accounts, &mut storage);

        // Try to join with insufficient stake
        let stake_amount = HclawAmount::from_hclaw(MIN_STAKE - 1);
        let result = contract.execute_join(&mut contract_state, sender, stake_amount);
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_join_twice() {
        let mut contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        accounts.insert(
            sender,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut contract_state = ContractState::new(&mut accounts, &mut storage);
        let stake_amount = HclawAmount::from_hclaw(MIN_STAKE);

        // First join succeeds
        assert!(contract
            .execute_join(&mut contract_state, sender, stake_amount)
            .is_ok());

        // Second join fails
        assert!(contract
            .execute_join(&mut contract_state, sender, stake_amount)
            .is_err());
    }
}
