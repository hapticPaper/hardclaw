//! Genesis Bounty Contract - manages participant onboarding and bounty distribution.
//!
//! This contract handles:
//! - Participant joining with minimum stake
//! - Flat 100 HCLAW airdrop distribution
//! - Parabolic daily bounty payouts
//! - Slot machine block winner selection
//! - Proportional reward distribution
//!
//! All mutable state is persisted in `ContractState` storage so that it
//! survives across calls (the `Contract::execute` trait method takes `&self`).

use serde::{Deserialize, Serialize};

use crate::contracts::state::ContractState;
use crate::contracts::transaction::ContractTransaction;
use crate::contracts::{Contract, ContractError, ContractResult, ExecutionResult};
use crate::crypto::Hash;
use crate::genesis::bounty::{
    distribute_bounty, is_winner_block, BountyTracker, BOUNTY_DAYS, MIN_PUBLIC_NODES,
};
use crate::genesis::DnsBreakGlassConfig;
use crate::types::{Address, HclawAmount};

/// Genesis configuration passed in init_data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenesisDeploymentConfig {
    /// Airdrop amount per participant
    pub airdrop_amount: HclawAmount,
    /// Maximum participants allowed
    pub max_participants: u32,
    /// Pre-approved addresses (skip competency/stake)
    pub pre_approved: Vec<Address>,
    /// DNS break-glass configuration
    pub dns_break_glass: DnsBreakGlassConfig,
    /// Bootstrap period end timestamp
    pub bootstrap_end: u64,
}

// ── Storage keys ────────────────────────────────────────────────────────────

const KEY_CONFIG: &[u8] = b"config";
const KEY_PARTICIPANT_COUNT: &[u8] = b"participant_count";
const KEY_BOUNTY_TRACKER: &[u8] = b"bounty_tracker";
/// Prefix for per-participant records: "participant:<hex address>"
const PARTICIPANT_PREFIX: &str = "participant:";

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
        /// Bootstrap day (0-89)
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

/// Genesis bounty contract — fully storage-backed.
///
/// The struct itself is stateless (only holds the fixed contract ID).
/// All mutable state lives in `ContractState` storage so it persists
/// across calls.
#[derive(Clone)]
pub struct GenesisBountyContract {
    /// Contract ID
    id: Hash,
}

impl GenesisBountyContract {
    /// Create new genesis bounty contract
    #[must_use]
    pub fn new(_start_time: i64) -> Self {
        Self {
            id: GENESIS_BOUNTY_CONTRACT_ID,
        }
    }

    /// Deterministic address derived from the contract ID
    fn address(&self) -> Address {
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&self.id.as_bytes()[..20]);
        Address::from_bytes(bytes)
    }

    // ── Storage helpers ─────────────────────────────────────────────────

    fn load_config(&self, state: &ContractState<'_>) -> ContractResult<GenesisDeploymentConfig> {
        let data = state
            .storage_read(&self.address(), KEY_CONFIG)
            .ok_or_else(|| {
                ContractError::ExecutionFailed("Contract not initialized".to_string())
            })?;
        bincode::deserialize(&data).map_err(|e| {
            ContractError::ExecutionFailed(format!("Config deserialization failed: {e}"))
        })
    }

    fn load_participant_count(&self, state: &ContractState<'_>) -> usize {
        state
            .storage_read(&self.address(), KEY_PARTICIPANT_COUNT)
            .and_then(|d| bincode::deserialize::<usize>(&d).ok())
            .unwrap_or(0)
    }

    fn save_participant_count(&self, state: &mut ContractState<'_>, count: usize) {
        let data = bincode::serialize(&count).expect("serialize usize");
        state.storage_write(self.address(), KEY_PARTICIPANT_COUNT.to_vec(), data);
    }

    fn participant_key(sender: &Address) -> Vec<u8> {
        format!("{}{}", PARTICIPANT_PREFIX, hex::encode(sender.as_bytes())).into_bytes()
    }

    fn load_participant(&self, state: &ContractState<'_>, sender: &Address) -> Option<Participant> {
        let key = Self::participant_key(sender);
        state
            .storage_read(&self.address(), &key)
            .and_then(|d| bincode::deserialize(&d).ok())
    }

    fn save_participant(
        &self,
        state: &mut ContractState<'_>,
        participant: &Participant,
    ) -> ContractResult<()> {
        let key = Self::participant_key(&participant.address);
        let data = bincode::serialize(participant)
            .map_err(|e| ContractError::ExecutionFailed(format!("Serialization failed: {e}")))?;
        state.storage_write(self.address(), key, data);
        Ok(())
    }

    fn load_bounty_tracker(&self, state: &ContractState<'_>) -> BountyTracker {
        state
            .storage_read(&self.address(), KEY_BOUNTY_TRACKER)
            .and_then(|d| bincode::deserialize::<BountyTracker>(&d).ok())
            .unwrap_or_else(|| BountyTracker::new(0))
    }

    fn save_bounty_tracker(
        &self,
        state: &mut ContractState<'_>,
        tracker: &BountyTracker,
    ) -> ContractResult<()> {
        let data = bincode::serialize(tracker)
            .map_err(|e| ContractError::ExecutionFailed(format!("Serialization failed: {e}")))?;
        state.storage_write(self.address(), KEY_BOUNTY_TRACKER.to_vec(), data);
        Ok(())
    }

    // ── Action handlers ─────────────────────────────────────────────────

    fn parse_action(input: &[u8]) -> ContractResult<BountyAction> {
        bincode::deserialize(input)
            .map_err(|e| ContractError::InvalidTransaction(format!("Failed to parse action: {e}")))
    }

    fn execute_join(
        &self,
        state: &mut ContractState<'_>,
        sender: Address,
        stake_amount: HclawAmount,
    ) -> ContractResult<()> {
        let config = self.load_config(state)?;

        // Check not already joined (via storage lookup)
        if self.load_participant(state, &sender).is_some() {
            return Err(ContractError::ExecutionFailed(
                "Already joined genesis".to_string(),
            ));
        }

        // Check participant limit
        let participant_count = self.load_participant_count(state);
        if participant_count >= config.max_participants as usize {
            return Err(ContractError::ExecutionFailed(
                "Maximum participants reached".to_string(),
            ));
        }

        // Validate stake (skip for pre-approved users)
        let is_pre_approved = config.pre_approved.contains(&sender);

        if !is_pre_approved {
            let min_stake = HclawAmount::from_hclaw(MIN_STAKE);
            if stake_amount < min_stake {
                return Err(ContractError::ExecutionFailed(format!(
                    "Stake {} below minimum {}",
                    stake_amount, min_stake
                )));
            }
            // Debit stake from participant
            state.debit(sender, stake_amount)?;
        }

        // Credit airdrop
        let airdrop = config.airdrop_amount;
        state.credit(sender, airdrop);

        // Persist participant
        let participant = Participant {
            address: sender,
            stake: stake_amount,
            airdrop,
            bounties_earned: HclawAmount::ZERO,
            joined_at: crate::types::now_millis() as u64,
        };
        self.save_participant(state, &participant)?;
        self.save_participant_count(state, participant_count + 1);

        // Emit event
        let event_data = bincode::serialize(&(sender, stake_amount)).unwrap();
        state.emit_event(crate::contracts::ContractEvent {
            contract_id: self.id,
            topic: "ParticipantJoined".to_string(),
            data: event_data,
        });

        Ok(())
    }

    fn execute_claim(
        &self,
        state: &mut ContractState<'_>,
        day: u8,
        block_hash: Hash,
        contributors: Vec<(Address, u32)>,
    ) -> ContractResult<()> {
        if day >= BOUNTY_DAYS {
            return Err(ContractError::ExecutionFailed(format!(
                "Day {} out of range",
                day
            )));
        }

        let mut tracker = self.load_bounty_tracker(state);

        if !tracker.is_active() {
            return Err(ContractError::ExecutionFailed(format!(
                "Bounties not active (need {} public nodes, have {})",
                MIN_PUBLIC_NODES, tracker.public_node_count
            )));
        }

        // Check if this block wins
        let threshold = u32::MAX / 10; // 10% win chance
        if !is_winner_block(block_hash, day, threshold) {
            return Err(ContractError::ExecutionFailed(
                "Block is not a winner".to_string(),
            ));
        }

        let remaining = tracker.remaining_today(day);
        if remaining.raw() == 0 {
            return Err(ContractError::ExecutionFailed(
                "No remaining budget for today".to_string(),
            ));
        }

        // Distribute bounty
        let distributions = distribute_bounty(contributors, remaining);

        // Apply distributions and update participant records
        let mut total_distributed = HclawAmount::ZERO;
        for (addr, amount) in &distributions {
            state.credit(*addr, *amount);
            total_distributed = total_distributed.saturating_add(*amount);

            // Update participant bounty tally in storage
            if let Some(mut participant) = self.load_participant(state, addr) {
                participant.bounties_earned = participant.bounties_earned.saturating_add(*amount);
                let _ = self.save_participant(state, &participant);
            }
        }

        // Record payout and persist tracker
        tracker.record_payout(day, total_distributed);
        self.save_bounty_tracker(state, &tracker)?;

        // Emit event
        let event_data = bincode::serialize(&(day, block_hash, total_distributed)).unwrap();
        state.emit_event(crate::contracts::ContractEvent {
            contract_id: self.id,
            topic: "BountyClaimed".to_string(),
            data: event_data,
        });

        Ok(())
    }

    fn execute_update_nodes(
        &self,
        state: &mut ContractState<'_>,
        count: u32,
    ) -> ContractResult<()> {
        let mut tracker = self.load_bounty_tracker(state);
        tracker.update_node_count(count);
        self.save_bounty_tracker(state, &tracker)
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
        let action = Self::parse_action(&tx.input)?;

        match action {
            BountyAction::JoinGenesis { stake } => {
                self.execute_join(state, tx.sender_address, stake)?;
            }
            BountyAction::ClaimBounty {
                day,
                block_hash,
                contributors,
            } => {
                self.execute_claim(state, day, block_hash, contributors)?;
            }
            BountyAction::UpdateNodeCount { count } => {
                self.execute_update_nodes(state, count)?;
            }
        }

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
        let computed_root = state.compute_state_root();
        Ok(computed_root == result.new_state_root)
    }

    fn is_upgradeable(&self) -> bool {
        false // Genesis contract is immutable
    }

    fn on_deploy(&self, state: &mut ContractState<'_>, init_data: &[u8]) -> ContractResult<()> {
        let config: GenesisDeploymentConfig = bincode::deserialize(init_data)
            .map_err(|e| ContractError::InvalidTransaction(format!("Invalid init_data: {e}")))?;

        // Store config
        state.storage_write(self.address(), KEY_CONFIG.to_vec(), init_data.to_vec());

        // Initialize participant count
        self.save_participant_count(state, 0);

        // Initialize bounty tracker with current time
        let now = crate::types::now_millis();
        let tracker = BountyTracker::new(now);
        self.save_bounty_tracker(state, &tracker)?;

        // Emit initialization event
        let event_data = bincode::serialize(&config.bootstrap_end).unwrap_or_default();
        state.emit_event(crate::contracts::ContractEvent {
            contract_id: self.id,
            topic: "Initialized".to_string(),
            data: event_data,
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::crypto::Keypair;

    /// Store a default GenesisDeploymentConfig into contract storage so
    /// `execute_join` can read it.
    fn store_default_config(
        contract: &GenesisBountyContract,
        storage: &mut HashMap<(Address, Vec<u8>), Vec<u8>>,
    ) {
        let authority_kp = Keypair::generate();
        let config = GenesisDeploymentConfig {
            airdrop_amount: HclawAmount::from_hclaw(AIRDROP_AMOUNT),
            max_participants: MAX_PARTICIPANTS as u32,
            pre_approved: Vec::new(),
            dns_break_glass: DnsBreakGlassConfig {
                domain: "bootstrap.hardclaw.net".to_string(),
                max_nodes: 10,
                tokens_each: HclawAmount::from_hclaw(500),
                vesting_ms: 86_400_000,
                authority_key: authority_kp.public_key().clone(),
            },
            bootstrap_end: 9_999_999_999,
        };
        let data = bincode::serialize(&config).expect("serialize config");
        storage.insert((contract.address(), KEY_CONFIG.to_vec()), data);
    }

    #[test]
    fn test_join_genesis() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        // Fund sender
        accounts.insert(
            sender,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        let stake_amount = HclawAmount::from_hclaw(MIN_STAKE);
        let result = contract.execute_join(&mut state, sender, stake_amount);
        assert!(result.is_ok(), "join failed: {:?}", result.err());

        // Verify via storage (not in-memory state)
        assert!(contract.load_participant(&state, &sender).is_some());
        assert_eq!(contract.load_participant_count(&state), 1);
    }

    #[test]
    fn test_join_fails_below_min_stake() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        accounts.insert(
            sender,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        let stake_amount = HclawAmount::from_hclaw(MIN_STAKE - 1);
        let result = contract.execute_join(&mut state, sender, stake_amount);
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_join_twice() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        accounts.insert(
            sender,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);
        let stake_amount = HclawAmount::from_hclaw(MIN_STAKE);

        // First join succeeds
        assert!(contract
            .execute_join(&mut state, sender, stake_amount)
            .is_ok());

        // Second join fails — duplicate detected via storage
        assert!(contract
            .execute_join(&mut state, sender, stake_amount)
            .is_err());
    }

    #[test]
    fn test_bounty_tracker_persists_across_calls() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let mut state = ContractState::new(&mut accounts, &mut storage);

        // Update node count — should persist via storage
        assert!(contract.execute_update_nodes(&mut state, 10).is_ok());

        // Load tracker back — count should be 10
        let tracker = contract.load_bounty_tracker(&state);
        assert_eq!(tracker.public_node_count, 10);
        assert!(tracker.is_active());
    }
}
