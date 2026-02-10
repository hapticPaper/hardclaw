//! Genesis Bounty Contract - manages participant onboarding and bounty distribution.
//!
//! This contract handles:
//! - Participant joining with minimum stake
//! - Tiered airdrop distribution (founders 250K, regular 100 HCLAW)
//! - Hourly bounty distribution to eligible staked verifiers
//! - Even distribution among attestation-eligible participants
//!
//! All mutable state is persisted in `ContractState` storage so that it
//! survives across calls (the `Contract::execute` trait method takes `&self`).

use serde::{Deserialize, Serialize};

use crate::contracts::state::ContractState;
use crate::contracts::transaction::ContractTransaction;
use crate::contracts::{Contract, ContractError, ContractResult, ExecutionResult};
use crate::crypto::Hash;
use crate::genesis::bounty::{
    calculate_hourly_budget, day_from_epoch, distribute_evenly, BountyTracker, MIN_PUBLIC_NODES,
};
use crate::genesis::DnsBreakGlassConfig;
use crate::types::{Address, HclawAmount};

/// Genesis configuration passed in init_data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenesisDeploymentConfig {
    /// Standard airdrop amount per participant (100 HCLAW)
    pub airdrop_amount: HclawAmount,
    /// Founder airdrop amount for pre-approved wallets (250,000 HCLAW)
    pub founder_airdrop_amount: HclawAmount,
    /// Maximum participants allowed
    pub max_participants: u32,
    /// Pre-approved addresses (founders — get founder_airdrop_amount, skip competency/stake)
    pub pre_approved: Vec<Address>,
    /// Bootstrap node addresses (get bootstrap_node_tokens at genesis)
    pub bootstrap_nodes: Vec<Address>,
    /// Tokens per bootstrap node (500,000 HCLAW)
    pub bootstrap_node_tokens: HclawAmount,
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
    /// Distribute hourly bounty to eligible staked verifiers.
    ///
    /// Injected as a system job by the block proposer at each hour boundary.
    /// All verifiers re-execute independently — if the proposer lies about the
    /// eligible list, state hashes diverge and the block is rejected.
    DistributeHourly {
        /// Hour index since bounty start (0–2159)
        epoch: u64,
        /// Staked verifiers who attested in the prior hour
        eligible_verifiers: Vec<Address>,
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

        // Validate stake (skip for pre-approved/founder users)
        let is_founder = config.pre_approved.contains(&sender);

        if !is_founder {
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

        // Credit airdrop: founders get 250K, everyone else gets 100
        let airdrop = if is_founder {
            config.founder_airdrop_amount
        } else {
            config.airdrop_amount
        };
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

    fn execute_distribute_hourly(
        &self,
        state: &mut ContractState<'_>,
        epoch: u64,
        eligible_verifiers: Vec<Address>,
    ) -> ContractResult<()> {
        let mut tracker = self.load_bounty_tracker(state);

        // 1. Check bounties active (enough public nodes)
        if !tracker.is_active() {
            return Err(ContractError::ExecutionFailed(format!(
                "Bounties not active (need {} public nodes, have {})",
                MIN_PUBLIC_NODES, tracker.public_node_count
            )));
        }

        // 2. Check sequential epoch ordering
        if !tracker.is_next_epoch(epoch) {
            return Err(ContractError::ExecutionFailed(format!(
                "Epoch {} is not the next expected epoch",
                epoch
            )));
        }

        // 3. Compute hourly budget
        let day = day_from_epoch(epoch);
        let hourly_budget = calculate_hourly_budget(day);

        // 4. If no eligible verifiers or zero budget, burn this hour's budget
        if eligible_verifiers.is_empty() || hourly_budget.raw() == 0 {
            tracker.record_distribution(epoch, HclawAmount::ZERO);
            tracker.record_burn(hourly_budget);
            self.save_bounty_tracker(state, &tracker)?;

            let event_data = bincode::serialize(&(epoch, day, hourly_budget)).unwrap();
            state.emit_event(crate::contracts::ContractEvent {
                contract_id: self.id,
                topic: "HourlyBountyBurned".to_string(),
                data: event_data,
            });
            return Ok(());
        }

        // 5. Validate every address is a joined participant with stake > 0
        for addr in &eligible_verifiers {
            match self.load_participant(state, addr) {
                None => {
                    return Err(ContractError::ExecutionFailed(format!(
                        "Address {} is not a participant",
                        hex::encode(addr.as_bytes())
                    )));
                }
                Some(p) if p.stake.raw() == 0 => {
                    return Err(ContractError::ExecutionFailed(format!(
                        "Address {} has zero stake",
                        hex::encode(addr.as_bytes())
                    )));
                }
                _ => {}
            }
        }

        // 6. Distribute evenly
        let distributions = distribute_evenly(&eligible_verifiers, hourly_budget);

        let mut total_distributed = HclawAmount::ZERO;
        for (addr, amount) in &distributions {
            state.credit(*addr, *amount);
            total_distributed = total_distributed.saturating_add(*amount);

            // Update participant bounty tally
            if let Some(mut participant) = self.load_participant(state, addr) {
                participant.bounties_earned = participant.bounties_earned.saturating_add(*amount);
                let _ = self.save_participant(state, &participant);
            }
        }

        // 7. Record distribution + dust burn
        let dust = HclawAmount::from_raw(hourly_budget.raw() - total_distributed.raw());
        tracker.record_distribution(epoch, total_distributed);
        if dust.raw() > 0 {
            tracker.record_burn(dust);
        }
        self.save_bounty_tracker(state, &tracker)?;

        // 8. Emit event
        let event_data = bincode::serialize(&(
            epoch,
            day,
            total_distributed,
            eligible_verifiers.len() as u32,
        ))
        .unwrap();
        state.emit_event(crate::contracts::ContractEvent {
            contract_id: self.id,
            topic: "HourlyBountyDistributed".to_string(),
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
            BountyAction::DistributeHourly {
                epoch,
                eligible_verifiers,
            } => {
                self.execute_distribute_hourly(state, epoch, eligible_verifiers)?;
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
        let now = crate::types::now_millis() as u64;
        let tracker = BountyTracker::new(now);
        self.save_bounty_tracker(state, &tracker)?;

        // NOTE: Initial balance allocations (bootstrap nodes, founders) are
        // applied via genesis_alloc in the genesis block — not here. This
        // on_deploy only initializes contract storage.

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
        store_config_with_founders(contract, storage, Vec::new());
    }

    /// Store config with specific founder addresses.
    fn store_config_with_founders(
        contract: &GenesisBountyContract,
        storage: &mut HashMap<(Address, Vec<u8>), Vec<u8>>,
        founders: Vec<Address>,
    ) {
        let authority_kp = Keypair::generate();
        let config = GenesisDeploymentConfig {
            airdrop_amount: HclawAmount::from_hclaw(AIRDROP_AMOUNT),
            founder_airdrop_amount: HclawAmount::from_hclaw(crate::genesis::FOUNDER_AIRDROP_AMOUNT),
            max_participants: MAX_PARTICIPANTS as u32,
            pre_approved: founders,
            bootstrap_nodes: Vec::new(),
            bootstrap_node_tokens: HclawAmount::from_hclaw(crate::genesis::BOOTSTRAP_NODE_TOKENS),
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

    /// Helper: create a joined participant with a given stake.
    fn join_participant(
        contract: &GenesisBountyContract,
        state: &mut ContractState<'_>,
        address: Address,
        stake: u64,
    ) {
        let participant = Participant {
            address,
            stake: HclawAmount::from_hclaw(stake),
            airdrop: HclawAmount::from_hclaw(100),
            bounties_earned: HclawAmount::ZERO,
            joined_at: 1_000_000,
        };
        contract.save_participant(state, &participant).unwrap();
    }

    /// Helper: set up bounty tracker with active node count and given start time.
    fn setup_active_tracker(
        contract: &GenesisBountyContract,
        state: &mut ContractState<'_>,
        start_time: u64,
    ) {
        let mut tracker = BountyTracker::new(start_time);
        tracker.update_node_count(MIN_PUBLIC_NODES);
        contract.save_bounty_tracker(state, &tracker).unwrap();
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

    #[test]
    fn test_founder_join_gets_250k() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        let kp = Keypair::generate();
        let founder = Address::from_public_key(kp.public_key());

        // Config with this address as a founder
        store_config_with_founders(&contract, &mut storage, vec![founder]);

        let mut state = ContractState::new(&mut accounts, &mut storage);

        // Founders don't need to stake
        let result = contract.execute_join(&mut state, founder, HclawAmount::ZERO);
        assert!(result.is_ok(), "founder join failed: {:?}", result.err());

        let participant = contract.load_participant(&state, &founder).unwrap();
        assert_eq!(participant.airdrop.whole_hclaw(), 250_000);
    }

    #[test]
    fn test_regular_join_gets_100() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let kp = Keypair::generate();
        let regular = Address::from_public_key(kp.public_key());

        accounts.insert(
            regular,
            crate::state::AccountState::new(HclawAmount::from_hclaw(1000)),
        );

        let mut state = ContractState::new(&mut accounts, &mut storage);

        let result = contract.execute_join(&mut state, regular, HclawAmount::from_hclaw(MIN_STAKE));
        assert!(result.is_ok(), "regular join failed: {:?}", result.err());

        let participant = contract.load_participant(&state, &regular).unwrap();
        assert_eq!(participant.airdrop.whole_hclaw(), 100);
    }

    // ── DistributeHourly tests ──────────────────────────────────────────

    #[test]
    fn test_distribute_hourly_credits_evenly() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let addrs: Vec<Address> = (0..3)
            .map(|i| {
                let mut b = [0u8; 20];
                b[0] = i + 1;
                Address::from_bytes(b)
            })
            .collect();

        let mut state = ContractState::new(&mut accounts, &mut storage);

        // Set up active tracker at epoch 0
        setup_active_tracker(&contract, &mut state, 0);

        // Register 3 participants with stake
        for addr in &addrs {
            join_participant(&contract, &mut state, *addr, MIN_STAKE);
        }

        // Distribute epoch 24 (day 1, hour 0 — first non-zero budget)
        // First advance tracker through epochs 0-23 (day 0, all zero budget)
        let mut tracker = contract.load_bounty_tracker(&state);
        for e in 0..24 {
            tracker.record_distribution(e, HclawAmount::ZERO);
        }
        contract.save_bounty_tracker(&mut state, &tracker).unwrap();

        let result = contract.execute_distribute_hourly(&mut state, 24, addrs.clone());
        assert!(result.is_ok(), "distribute failed: {:?}", result.err());

        // All 3 should have equal bounties_earned
        let p0 = contract.load_participant(&state, &addrs[0]).unwrap();
        let p1 = contract.load_participant(&state, &addrs[1]).unwrap();
        let p2 = contract.load_participant(&state, &addrs[2]).unwrap();
        assert_eq!(p0.bounties_earned, p1.bounties_earned);
        assert_eq!(p1.bounties_earned, p2.bounties_earned);
        assert!(p0.bounties_earned.raw() > 0, "Should have received bounty");
    }

    #[test]
    fn test_distribute_hourly_rejects_wrong_epoch() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let addr = Address::from_bytes([1; 20]);
        let mut state = ContractState::new(&mut accounts, &mut storage);

        setup_active_tracker(&contract, &mut state, 0);
        join_participant(&contract, &mut state, addr, MIN_STAKE);

        // Epoch 5 should fail — epoch 0 is next
        let result = contract.execute_distribute_hourly(&mut state, 5, vec![addr]);
        assert!(result.is_err());
        assert!(format!("{:?}", result.err().unwrap()).contains("not the next expected epoch"),);
    }

    #[test]
    fn test_distribute_hourly_burns_if_no_eligible() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let mut state = ContractState::new(&mut accounts, &mut storage);
        setup_active_tracker(&contract, &mut state, 0);

        // Distribute epoch 0 with empty verifier list — should burn
        let result = contract.execute_distribute_hourly(&mut state, 0, vec![]);
        assert!(result.is_ok());

        // Tracker should advance to epoch 0
        let tracker = contract.load_bounty_tracker(&state);
        assert_eq!(tracker.last_distributed_epoch, 0);
        assert_eq!(tracker.total_paid.raw(), 0);
    }

    #[test]
    fn test_distribute_hourly_rejects_non_participant() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let unknown = Address::from_bytes([99; 20]);
        let mut state = ContractState::new(&mut accounts, &mut storage);

        setup_active_tracker(&contract, &mut state, 0);

        // Advance past day 0 (zero budget) to epoch 24 (day 1)
        let mut tracker = contract.load_bounty_tracker(&state);
        for e in 0..24 {
            tracker.record_distribution(e, HclawAmount::ZERO);
        }
        contract.save_bounty_tracker(&mut state, &tracker).unwrap();

        // Unknown address — not joined
        let result = contract.execute_distribute_hourly(&mut state, 24, vec![unknown]);
        assert!(result.is_err());
        assert!(format!("{:?}", result.err().unwrap()).contains("not a participant"),);
    }

    #[test]
    fn test_distribute_hourly_rejects_zero_stake() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let addr = Address::from_bytes([1; 20]);
        let mut state = ContractState::new(&mut accounts, &mut storage);

        setup_active_tracker(&contract, &mut state, 0);

        // Advance past day 0 (zero budget) to epoch 24 (day 1)
        let mut tracker = contract.load_bounty_tracker(&state);
        for e in 0..24 {
            tracker.record_distribution(e, HclawAmount::ZERO);
        }
        contract.save_bounty_tracker(&mut state, &tracker).unwrap();

        // Join with zero stake
        join_participant(&contract, &mut state, addr, 0);

        let result = contract.execute_distribute_hourly(&mut state, 24, vec![addr]);
        assert!(result.is_err());
        assert!(format!("{:?}", result.err().unwrap()).contains("zero stake"),);
    }

    #[test]
    fn test_distribute_hourly_not_active_rejects() {
        let contract = GenesisBountyContract::new(1000);
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();

        store_default_config(&contract, &mut storage);

        let addr = Address::from_bytes([1; 20]);
        let mut state = ContractState::new(&mut accounts, &mut storage);

        // Tracker with 0 nodes — not active
        let tracker = BountyTracker::new(0);
        contract.save_bounty_tracker(&mut state, &tracker).unwrap();

        join_participant(&contract, &mut state, addr, MIN_STAKE);

        let result = contract.execute_distribute_hourly(&mut state, 0, vec![addr]);
        assert!(result.is_err());
        assert!(format!("{:?}", result.err().unwrap()).contains("not active"),);
    }
}
