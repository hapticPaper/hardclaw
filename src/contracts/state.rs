//! Contract state interface - provides controlled access to chain state.
//!
//! Contracts interact with the blockchain through `ContractState`, which:
//! - Provides read/write access to account balances
//! - Manages stakes and rewards
//! - Tracks contract-specific storage
//! - Ensures atomicity of state mutations

use std::collections::HashMap;

use crate::crypto::Hash;
use crate::state::AccountState;
use crate::types::{Address, HclawAmount};

/// State interface for contract execution
///
/// This wraps the full chain state and provides controlled access
/// to prevent contracts from corrupting state.
#[derive(Debug)]
pub struct ContractState<'a> {
    /// Account balances and metadata
    pub accounts: &'a mut HashMap<Address, AccountState>,
    /// Contract-specific key-value storage
    pub storage: &'a mut HashMap<(Address, Vec<u8>), Vec<u8>>,
    /// Pending state mutations (for atomic commit/rollback)
    mutations: Vec<StateMutation>,
    /// Events emitted during execution
    events: Vec<super::ContractEvent>,
}

/// Represents a state mutation that can be rolled back
#[derive(Clone, Debug)]
enum StateMutation {
    /// Account balance credit
    Credit {
        /// Account address
        address: Address,
        /// Amount credited
        amount: HclawAmount,
    },
    /// Account balance debit
    Debit {
        /// Account address
        address: Address,
        /// Amount debited
        amount: HclawAmount,
    },
    /// Storage write
    StorageWrite {
        /// Contract address
        contract: Address,
        /// Storage key
        key: Vec<u8>,
        /// Old value (for rollback)
        old_value: Option<Vec<u8>>,
        /// New value
        new_value: Vec<u8>,
    },
}

impl<'a> ContractState<'a> {
    /// Create new contract state wrapper
    #[must_use]
    pub fn new(
        accounts: &'a mut HashMap<Address, AccountState>,
        storage: &'a mut HashMap<(Address, Vec<u8>), Vec<u8>>,
    ) -> Self {
        Self {
            accounts,
            storage,
            mutations: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Get account balance
    #[must_use]
    pub fn balance(&self, address: &Address) -> HclawAmount {
        self.accounts
            .get(address)
            .map_or(HclawAmount::ZERO, |a| a.balance)
    }

    /// Get available balance (not staked)
    #[must_use]
    pub fn available_balance(&self, address: &Address) -> HclawAmount {
        self.accounts
            .get(address)
            .map_or(HclawAmount::ZERO, |a| a.available_balance())
    }

    /// Credit an account
    ///
    /// This queues a mutation that will be applied on commit.
    pub fn credit(&mut self, address: Address, amount: HclawAmount) {
        // Apply immediately
        let account = self.accounts.entry(address).or_default();
        account.credit(amount);

        // Record mutation for potential rollback
        self.mutations.push(StateMutation::Credit { address, amount });
    }

    /// Debit an account
    ///
    /// # Errors
    /// Returns error if insufficient balance
    pub fn debit(&mut self, address: Address, amount: HclawAmount) -> Result<(), super::ContractError> {
        // Validate balance
        let account = self.accounts.entry(address).or_default();
        if account.available_balance() < amount {
            return Err(super::ContractError::InsufficientBalance {
                need: amount,
                have: account.available_balance(),
            });
        }

        // Apply debit
        account.debit(amount).map_err(|e| {
            super::ContractError::ExecutionFailed(format!("debit failed: {}", e))
        })?;

        // Record mutation
        self.mutations.push(StateMutation::Debit { address, amount });

        Ok(())
    }

    /// Transfer tokens between accounts
    ///
    /// # Errors
    /// Returns error if insufficient balance
    pub fn transfer(
        &mut self,
        from: Address,
        to: Address,
        amount: HclawAmount,
    ) -> Result<(), super::ContractError> {
        self.debit(from, amount)?;
        self.credit(to, amount);
        Ok(())
    }

    /// Read from contract storage
    #[must_use]
    pub fn storage_read(&self, contract: &Address, key: &[u8]) -> Option<Vec<u8>> {
        self.storage.get(&(*contract, key.to_vec())).cloned()
    }

    /// Write to contract storage
    pub fn storage_write(&mut self, contract: Address, key: Vec<u8>, value: Vec<u8>) {
        let old_value = self.storage.get(&(contract, key.clone())).cloned();
        self.storage.insert((contract, key.clone()), value.clone());

        self.mutations.push(StateMutation::StorageWrite {
            contract,
            key,
            old_value,
            new_value: value,
        });
    }

    /// Delete from contract storage
    pub fn storage_delete(&mut self, contract: Address, key: Vec<u8>) {
        let old_value = self.storage.remove(&(contract, key.clone()));
        if let Some(old_val) = old_value {
            self.mutations.push(StateMutation::StorageWrite {
                contract,
                key,
                old_value: Some(old_val),
                new_value: Vec::new(),
            });
        }
    }

    /// Emit an event
    pub fn emit_event(&mut self, event: super::ContractEvent) {
        self.events.push(event);
    }

    /// Get all emitted events
    #[must_use]
    pub fn events(&self) -> &[super::ContractEvent] {
        &self.events
    }

    /// Commit all pending mutations
    ///
    /// This finalizes the state changes. After commit, rollback is no longer possible.
    pub fn commit(&mut self) {
        // Mutations are already applied, just clear the log
        self.mutations.clear();
    }

    /// Rollback all pending mutations
    ///
    /// This reverts all state changes made during contract execution.
    /// Used when execution fails or verification rejects the result.
    pub fn rollback(&mut self) {
        // Reverse mutations in reverse order
        for mutation in self.mutations.drain(..).rev() {
            match mutation {
                StateMutation::Credit { address, amount } => {
                    // Reverse credit = debit
                    if let Some(account) = self.accounts.get_mut(&address) {
                        account.balance = account.balance.saturating_sub(amount);
                    }
                }
                StateMutation::Debit { address, amount } => {
                    // Reverse debit = credit
                    if let Some(account) = self.accounts.get_mut(&address) {
                        account.balance = account.balance.saturating_add(amount);
                    }
                }
                StateMutation::StorageWrite {
                    contract,
                    key,
                    old_value,
                    ..
                } => {
                    // Restore old value (or delete if it didn't exist)
                    if let Some(old_val) = old_value {
                        self.storage.insert((contract, key), old_val);
                    } else {
                        self.storage.remove(&(contract, key));
                    }
                }
            }
        }

        // Clear events
        self.events.clear();
    }

    /// Compute state root hash
    ///
    /// This is used to verify state transitions match across verifiers.
    #[must_use]
    pub fn compute_state_root(&self) -> Hash {
        use crate::crypto::{hash_data, merkle_root};

        let mut hashes: Vec<Hash> = self
            .accounts
            .iter()
            .map(|(addr, state)| {
                let mut data = Vec::new();
                data.extend_from_slice(addr.as_bytes());
                data.extend_from_slice(&state.balance.raw().to_le_bytes());
                data.extend_from_slice(&state.nonce.to_le_bytes());
                data.extend_from_slice(&state.staked.raw().to_le_bytes());
                hash_data(&data)
            })
            .collect();

        hashes.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        merkle_root(&hashes)
    }
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
    fn test_credit_debit() {
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();
        let mut state = ContractState::new(&mut accounts, &mut storage);

        let addr = test_address();

        // Credit account
        state.credit(addr, HclawAmount::from_hclaw(100));
        assert_eq!(state.balance(&addr).whole_hclaw(), 100);

        // Debit account
        state.debit(addr, HclawAmount::from_hclaw(30)).unwrap();
        assert_eq!(state.balance(&addr).whole_hclaw(), 70);
    }

    #[test]
    fn test_rollback() {
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();
        let mut state = ContractState::new(&mut accounts, &mut storage);

        let addr = test_address();

        // Make changes
        state.credit(addr, HclawAmount::from_hclaw(100));
        assert_eq!(state.balance(&addr).whole_hclaw(), 100);

        // Rollback
        state.rollback();
        assert_eq!(state.balance(&addr).whole_hclaw(), 0);
    }

    #[test]
    fn test_transfer() {
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();
        let mut state = ContractState::new(&mut accounts, &mut storage);

        let alice = test_address();
        let bob = test_address();

        // Give Alice tokens
        state.credit(alice, HclawAmount::from_hclaw(100));

        // Transfer to Bob
        state.transfer(alice, bob, HclawAmount::from_hclaw(30)).unwrap();

        assert_eq!(state.balance(&alice).whole_hclaw(), 70);
        assert_eq!(state.balance(&bob).whole_hclaw(), 30);
    }

    #[test]
    fn test_storage() {
        let mut accounts = HashMap::new();
        let mut storage = HashMap::new();
        let mut state = ContractState::new(&mut accounts, &mut storage);

        let contract = test_address();
        let key = b"counter".to_vec();
        let value = b"42".to_vec();

        // Write
        state.storage_write(contract, key.clone(), value.clone());
        assert_eq!(state.storage_read(&contract, &key), Some(value));

        // Rollback should revert
        state.rollback();
        assert_eq!(state.storage_read(&contract, &key), None);
    }
}
