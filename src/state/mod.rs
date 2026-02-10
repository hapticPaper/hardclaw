//! Blockchain state management.
//!
//! Tracks account balances, job states, and chain history.

use std::collections::HashMap;

use tracing::info;

use crate::crypto::{hash_data, merkle_root, Hash};
use crate::types::{
    Address, Block, HclawAmount, Id, JobPacket, JobType, SolutionCandidate, SystemJobKind,
    VerificationSpec,
};

/// Account state
#[derive(Clone, Debug, Default)]
pub struct AccountState {
    /// Account balance
    pub balance: HclawAmount,
    /// Nonce (for transaction ordering)
    pub nonce: u64,
    /// Staked amount
    pub staked: HclawAmount,
    /// Total rewards earned
    pub total_rewards: HclawAmount,
    /// Total spent on jobs
    pub total_spent: HclawAmount,
    /// Total earned from solving
    pub total_earned: HclawAmount,
}

impl AccountState {
    /// Create a new account with initial balance
    #[must_use]
    pub const fn new(balance: HclawAmount) -> Self {
        Self {
            balance,
            nonce: 0,
            staked: HclawAmount::ZERO,
            total_rewards: HclawAmount::ZERO,
            total_spent: HclawAmount::ZERO,
            total_earned: HclawAmount::ZERO,
        }
    }

    /// Get available balance (not staked)
    #[must_use]
    pub fn available_balance(&self) -> HclawAmount {
        self.balance.saturating_sub(self.staked)
    }

    /// Credit balance
    pub fn credit(&mut self, amount: HclawAmount) {
        self.balance = self.balance.saturating_add(amount);
    }

    /// Debit balance
    ///
    /// # Errors
    /// Returns error if insufficient balance
    pub fn debit(&mut self, amount: HclawAmount) -> Result<(), StateError> {
        if self.available_balance() < amount {
            return Err(StateError::InsufficientBalance {
                have: self.available_balance(),
                need: amount,
            });
        }

        self.balance = self.balance.saturating_sub(amount);
        Ok(())
    }
}

/// Chain state snapshot
///
/// Note: Custom Clone implementation creates a new empty TransactionProcessor
/// since ContractRegistry cannot be cloned (contains trait objects)
pub struct ChainState {
    /// Account states
    accounts: HashMap<Address, AccountState>,
    /// Block headers by hash
    blocks: HashMap<Hash, Block>,
    /// Block hash by height
    height_index: HashMap<u64, Hash>,
    /// Current chain tip
    tip: Option<Hash>,
    /// Current height
    height: u64,
    /// Jobs by ID
    jobs: HashMap<Id, JobPacket>,
    /// Solutions by ID
    solutions: HashMap<Id, SolutionCandidate>,
    /// Chain ID
    chain_id: Option<String>,
    /// Contract storage (contract_address, key) -> value
    contract_storage: HashMap<(Address, Vec<u8>), Vec<u8>>,
    /// Contract transaction processor
    tx_processor: crate::contracts::processor::TransactionProcessor,
}

impl Default for ChainState {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainState {
    /// Create new empty state
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            blocks: HashMap::new(),
            height_index: HashMap::new(),
            tip: None,
            height: 0,
            jobs: HashMap::new(),
            solutions: HashMap::new(),
            chain_id: None,
            contract_storage: HashMap::new(),
            tx_processor: crate::contracts::processor::TransactionProcessor::default(),
        }
    }

    /// Get chain ID
    #[must_use]
    pub fn chain_id(&self) -> Option<&str> {
        self.chain_id.as_deref()
    }

    /// Set chain ID
    pub fn set_chain_id(&mut self, chain_id: String) {
        self.chain_id = Some(chain_id);
    }

    /// Get or create account state
    pub fn get_or_create_account(&mut self, address: &Address) -> &mut AccountState {
        self.accounts.entry(*address).or_default()
    }

    /// Get account state
    #[must_use]
    pub fn get_account(&self, address: &Address) -> Option<&AccountState> {
        self.accounts.get(address)
    }

    /// Get account balance
    #[must_use]
    pub fn balance_of(&self, address: &Address) -> HclawAmount {
        self.accounts
            .get(address)
            .map_or(HclawAmount::ZERO, |a| a.balance)
    }

    /// Transfer tokens between accounts
    ///
    /// # Errors
    /// Returns error if sender has insufficient balance
    pub fn transfer(
        &mut self,
        from: &Address,
        to: &Address,
        amount: HclawAmount,
    ) -> Result<(), StateError> {
        // Debit from sender
        self.get_or_create_account(from).debit(amount)?;

        // Credit to receiver
        self.get_or_create_account(to).credit(amount);

        Ok(())
    }

    /// Apply a block to the state
    ///
    /// # Errors
    /// Returns error if block is invalid
    pub fn apply_block(&mut self, block: Block) -> Result<(), StateError> {
        // Verify block follows current tip
        if let Some(tip) = &self.tip {
            if block.header.parent_hash != *tip {
                return Err(StateError::InvalidParent);
            }

            if block.header.height != self.height + 1 {
                return Err(StateError::InvalidHeight {
                    expected: self.height + 1,
                    got: block.header.height,
                });
            }
        } else if block.header.height != 0 {
            return Err(StateError::InvalidHeight {
                expected: 0,
                got: block.header.height,
            });
        }

        // Genesis block: apply initial allocations and deploy contracts
        if block.header.height == 0 {
            // 1. Apply genesis allocations directly to state (like Ethereum alloc)
            for alloc in &block.genesis_alloc {
                self.get_or_create_account(&alloc.address)
                    .credit(alloc.amount);
                info!(
                    "Genesis alloc: {} HCLAW → {} ({})",
                    alloc.amount.whole_hclaw(),
                    alloc.address,
                    alloc.label
                );
            }

            // 2. Deploy genesis contract (initializes contract storage only — no credits)
            if let Some(ref job) = block.genesis_job {
                if matches!(job.job_type, JobType::System) {
                    if let VerificationSpec::SystemOperation {
                        kind:
                            SystemJobKind::DeployContract {
                                code,
                                init_data,
                                deployer: _,
                            },
                        ..
                    } = &job.verification
                    {
                        info!("Deploying genesis contract...");
                        let tx_kind = crate::contracts::transaction::TransactionKind::Deploy {
                            code: code.clone(),
                            init_data: init_data.clone(),
                            deployer: job.requester_address,
                        };

                        if let Err(e) = self.tx_processor.process_transaction(
                            &tx_kind,
                            &mut self.accounts,
                            &mut self.contract_storage,
                        ) {
                            tracing::error!("Failed to deploy genesis contract: {}", e);
                            return Err(StateError::InvalidParent);
                        }
                    }
                }
            }
        }

        // Store block indexed by its actual header height
        let block_height = block.header.height;
        let block_hash = block.hash;
        self.blocks.insert(block_hash, block);
        self.height_index.insert(block_height, block_hash);
        self.tip = Some(block_hash);
        self.height = block_height + 1;

        Ok(())
    }

    /// Get block by hash
    #[must_use]
    pub fn get_block(&self, hash: &Hash) -> Option<&Block> {
        self.blocks.get(hash)
    }

    /// Get block by height
    #[must_use]
    pub fn get_block_at_height(&self, height: u64) -> Option<&Block> {
        self.height_index
            .get(&height)
            .and_then(|h| self.blocks.get(h))
    }

    /// Get current tip
    #[must_use]
    pub fn tip(&self) -> Option<&Block> {
        self.tip.and_then(|h| self.blocks.get(&h))
    }

    /// Get current height
    #[must_use]
    pub const fn height(&self) -> u64 {
        self.height
    }

    /// Compute state root (merkle root of all account states)
    #[must_use]
    pub fn compute_state_root(&self) -> Hash {
        let mut hashes: Vec<Hash> = self
            .accounts
            .iter()
            .map(|(addr, state)| {
                let mut data = Vec::new();
                data.extend_from_slice(addr.as_bytes());
                data.extend_from_slice(&state.balance.raw().to_le_bytes());
                data.extend_from_slice(&state.nonce.to_le_bytes());
                hash_data(&data)
            })
            .collect();

        hashes.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        merkle_root(&hashes)
    }

    /// Store a job
    pub fn store_job(&mut self, job: JobPacket) {
        self.jobs.insert(job.id, job);
    }

    /// Get job by ID
    #[must_use]
    pub fn get_job(&self, id: &Id) -> Option<&JobPacket> {
        self.jobs.get(id)
    }

    /// Store a solution
    pub fn store_solution(&mut self, solution: SolutionCandidate) {
        self.solutions.insert(solution.id, solution);
    }

    /// Get solution by ID
    #[must_use]
    pub fn get_solution(&self, id: &Id) -> Option<&SolutionCandidate> {
        self.solutions.get(id)
    }

    /// Execute a contract transaction
    ///
    /// This is the enforcement mechanism - verified transactions
    /// are executed and state changes are applied atomically.
    ///
    /// # Errors
    /// Returns error if execution fails
    pub fn execute_contract(
        &mut self,
        contract: &dyn crate::contracts::Contract,
        transaction: &crate::contracts::transaction::ContractTransaction,
    ) -> Result<crate::contracts::ExecutionResult, crate::contracts::ContractError> {
        self.tx_processor.execute_transaction(
            contract,
            transaction,
            &mut self.accounts,
            &mut self.contract_storage,
        )
    }

    /// Verify a contract execution result
    ///
    /// Independent verification without modifying state.
    ///
    /// # Errors
    /// Returns error if verification logic fails
    pub fn verify_contract_execution(
        &self,
        contract: &dyn crate::contracts::Contract,
        transaction: &crate::contracts::transaction::ContractTransaction,
        result: &crate::contracts::ExecutionResult,
    ) -> Result<bool, crate::contracts::ContractError> {
        self.tx_processor.verify_execution(
            contract,
            transaction,
            result,
            &self.accounts,
            &self.contract_storage,
        )
    }

    /// Get contract storage value
    #[must_use]
    pub fn get_contract_storage(&self, contract: &Address, key: &[u8]) -> Option<Vec<u8>> {
        self.contract_storage
            .get(&(*contract, key.to_vec()))
            .cloned()
    }
}

/// State errors
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// Insufficient balance
    #[error("insufficient balance: have {have}, need {need}")]
    InsufficientBalance {
        /// Current balance
        have: HclawAmount,
        /// Amount needed
        need: HclawAmount,
    },
    /// Invalid parent block
    #[error("invalid parent block")]
    InvalidParent,
    /// Invalid block height
    #[error("invalid height: expected {expected}, got {got}")]
    InvalidHeight {
        /// Expected block height
        expected: u64,
        /// Actual block height
        got: u64,
    },
    /// Block not found
    #[error("block not found")]
    BlockNotFound,
    /// Account not found
    #[error("account not found")]
    AccountNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_address() -> Address {
        let kp = Keypair::generate();
        Address::from_public_key(kp.public_key())
    }

    #[test]
    fn test_account_state() {
        let mut account = AccountState::new(HclawAmount::from_hclaw(100));

        assert_eq!(account.balance.whole_hclaw(), 100);

        account.debit(HclawAmount::from_hclaw(30)).unwrap();
        assert_eq!(account.balance.whole_hclaw(), 70);

        account.credit(HclawAmount::from_hclaw(50));
        assert_eq!(account.balance.whole_hclaw(), 120);
    }

    #[test]
    fn test_insufficient_balance() {
        let mut account = AccountState::new(HclawAmount::from_hclaw(10));

        let result = account.debit(HclawAmount::from_hclaw(100));
        assert!(matches!(
            result,
            Err(StateError::InsufficientBalance { .. })
        ));
    }

    #[test]
    fn test_transfer() {
        let mut state = ChainState::new();

        let alice = test_address();
        let bob = test_address();

        // Give Alice some tokens
        state
            .get_or_create_account(&alice)
            .credit(HclawAmount::from_hclaw(100));

        // Transfer to Bob
        state
            .transfer(&alice, &bob, HclawAmount::from_hclaw(30))
            .unwrap();

        assert_eq!(state.balance_of(&alice).whole_hclaw(), 70);
        assert_eq!(state.balance_of(&bob).whole_hclaw(), 30);
    }

    #[test]
    fn test_block_application() {
        let mut state = ChainState::new();
        let kp = Keypair::generate();

        // Apply genesis block
        let genesis = Block::genesis(kp.public_key().clone());
        state.apply_block(genesis).unwrap();

        assert_eq!(state.height(), 1);
        assert!(state.tip().is_some());
    }
}

/// Custom Clone implementation
/// Creates a new TransactionProcessor since contracts can't be cloned
impl Clone for ChainState {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            blocks: self.blocks.clone(),
            height_index: self.height_index.clone(),
            tip: self.tip,
            height: self.height,
            jobs: self.jobs.clone(),
            solutions: self.solutions.clone(),
            chain_id: self.chain_id.clone(),
            contract_storage: self.contract_storage.clone(),
            // Create a fresh transaction processor (contracts can't be cloned)
            tx_processor: crate::contracts::processor::TransactionProcessor::default(),
        }
    }
}
