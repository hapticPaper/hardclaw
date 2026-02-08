//! Transaction processor - executes contracts and applies state transitions.
//!
//! This is the **enforcement mechanism** that makes contracts binding.
//! It takes verified transactions, executes contract logic, and atomically
//! applies state changes to the blockchain.

use std::collections::HashMap;

use tracing::{debug, error, info};

use super::{Contract, ContractError, ContractResult, ExecutionResult};
use crate::contracts::state::ContractState;
use crate::contracts::transaction::ContractTransaction;
use crate::state::AccountState;
use crate::types::Address;

/// Processes contract transactions and applies state transitions
pub struct TransactionProcessor {
    /// Maximum gas per transaction
    max_gas: u64,
    /// Contract registry for looking up contracts
    registry: crate::contracts::ContractRegistry,
}

impl TransactionProcessor {
    /// Create new transaction processor with empty registry
    #[must_use]
    pub fn new(max_gas: u64) -> Self {
        Self {
            max_gas,
            registry: crate::contracts::ContractRegistry::new(),
        }
    }

    /// Create transaction processor with pre-configured registry
    #[must_use]
    pub fn with_registry(max_gas: u64, registry: crate::contracts::ContractRegistry) -> Self {
        Self { max_gas, registry }
    }

    /// Get reference to contract registry
    pub fn registry(&self) -> &crate::contracts::ContractRegistry {
        &self.registry
    }

    /// Execute a contract transaction
    ///
    /// This is the core enforcement mechanism:
    /// 1. Validates transaction (signature, nonce, gas)
    /// 2. Creates contract state wrapper
    /// 3. Executes contract logic
    /// 4. On success: commits state changes
    /// 5. On failure: rolls back all changes
    ///
    /// # Errors
    /// Returns error if transaction is invalid or execution fails
    pub fn execute_transaction(
        &self,
        contract: &dyn Contract,
        tx: &ContractTransaction,
        accounts: &mut HashMap<Address, AccountState>,
        storage: &mut HashMap<(Address, Vec<u8>), Vec<u8>>,
    ) -> ContractResult<ExecutionResult> {
        // Validate transaction
        self.validate_transaction(tx, accounts)?;

        // Create contract state wrapper
        let mut state = ContractState::new(accounts, storage);

        // Execute contract logic
        debug!(
            contract_id = %contract.id(),
            tx_id = %tx.id,
            "Executing contract transaction"
        );

        let result = contract.execute(&mut state, tx);

        match result {
            Ok(exec_result) => {
                // Verify state root matches
                let computed_root = state.compute_state_root();
                if computed_root != exec_result.new_state_root {
                    error!(
                        expected = %exec_result.new_state_root,
                        got = %computed_root,
                        "State root mismatch"
                    );
                    state.rollback();
                    return Err(ContractError::StateRootMismatch {
                        expected: exec_result.new_state_root,
                        got: computed_root,
                    });
                }

                // Commit state changes
                state.commit();

                info!(
                    contract_id = %contract.id(),
                    tx_id = %tx.id,
                    gas_used = exec_result.gas_used,
                    events = exec_result.events.len(),
                    "Contract execution successful"
                );

                Ok(exec_result)
            }
            Err(e) => {
                error!(
                    contract_id = %contract.id(),
                    tx_id = %tx.id,
                    error = %e,
                    "Contract execution failed"
                );
                // Rollback all state changes
                state.rollback();
                Err(e)
            }
        }
    }

    /// Verify a proposed execution result
    ///
    /// Other verifiers call this to independently verify that:
    /// 1. The transaction is valid
    /// 2. The execution result is correct
    /// 3. The state root matches
    ///
    /// This enables consensus without trust.
    pub fn verify_execution(
        &self,
        contract: &dyn Contract,
        tx: &ContractTransaction,
        result: &ExecutionResult,
        accounts: &HashMap<Address, AccountState>,
        storage: &HashMap<(Address, Vec<u8>), Vec<u8>>,
    ) -> ContractResult<bool> {
        // Validate transaction
        self.validate_transaction(tx, accounts)?;

        // Create read-only state snapshot
        let mut accounts_copy = accounts.clone();
        let mut storage_copy = storage.clone();
        let state = ContractState::new(&mut accounts_copy, &mut storage_copy);

        // Verify using contract's verify method
        let is_valid = contract.verify(&state, tx, result)?;

        if !is_valid {
            debug!(
                contract_id = %contract.id(),
                tx_id = %tx.id,
                "Verification failed: contract rejected result"
            );
            return Ok(false);
        }

        // Verify state root
        let computed_root = state.compute_state_root();
        if computed_root != result.new_state_root {
            debug!(
                expected = %result.new_state_root,
                got = %computed_root,
                "Verification failed: state root mismatch"
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// Validate transaction before execution
    fn validate_transaction(
        &self,
        tx: &ContractTransaction,
        accounts: &HashMap<Address, AccountState>,
    ) -> ContractResult<()> {
        // Verify signature
        tx.verify_signature()
            .map_err(|e| ContractError::InvalidTransaction(format!("Invalid signature: {}", e)))?;

        // Check gas limit
        if tx.gas_limit > self.max_gas {
            return Err(ContractError::InvalidTransaction(format!(
                "Gas limit {} exceeds maximum {}",
                tx.gas_limit, self.max_gas
            )));
        }

        // Check sender has funds for max fee
        let max_fee = tx.max_fee();
        let sender_balance = accounts
            .get(&tx.sender_address)
            .map_or(crate::types::HclawAmount::ZERO, |a| a.available_balance());

        if sender_balance < max_fee {
            return Err(ContractError::InsufficientBalance {
                need: max_fee,
                have: sender_balance,
            });
        }

        // Check nonce (should be sender's current nonce + 1)
        let expected_nonce = accounts
            .get(&tx.sender_address)
            .map_or(0, |a| a.nonce + 1);

        if tx.nonce != expected_nonce {
            return Err(ContractError::InvalidTransaction(format!(
                "Invalid nonce: expected {}, got {}",
                expected_nonce, tx.nonce
            )));
        }

        Ok(())
    }

    /// Batch process multiple transactions atomically
    ///
    /// Either all transactions succeed or all fail.
    /// This enables atomic multi-contract operations.
    pub fn execute_batch(
        &self,
        transactions: &[(Box<dyn Contract>, ContractTransaction)],
        accounts: &mut HashMap<Address, AccountState>,
        storage: &mut HashMap<(Address, Vec<u8>), Vec<u8>>,
    ) -> ContractResult<Vec<ExecutionResult>> {
        // Clone state for rollback
        let accounts_backup = accounts.clone();
        let storage_backup = storage.clone();

        let mut results = Vec::new();

        for (contract, tx) in transactions {
            match self.execute_transaction(contract.as_ref(), tx, accounts, storage) {
                Ok(result) => results.push(result),
                Err(e) => {
                    // Rollback entire batch
                    *accounts = accounts_backup;
                    *storage = storage_backup;
                    return Err(e);
                }
            }
        }

        Ok(results)
    }
}

impl Default for TransactionProcessor {
    fn default() -> Self {
        // Default max gas: 10 million units
        Self::new(10_000_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;
    use crate::types::{HclawAmount, Id};

    // Mock contract for testing
    struct MockContract {
        id: Id,
    }

    impl Contract for MockContract {
        fn id(&self) -> Id {
            self.id
        }

        fn name(&self) -> &str {
            "MockContract"
        }

        fn version(&self) -> u32 {
            1
        }

        fn execute(
            &self,
            state: &mut ContractState<'_>,
            tx: &ContractTransaction,
        ) -> ContractResult<ExecutionResult> {
            // Simple transfer for testing
            let recipient = Address::from_bytes([1; 20]);
            state.transfer(tx.sender_address, recipient, HclawAmount::from_hclaw(10))?;

            Ok(ExecutionResult {
                new_state_root: state.compute_state_root(),
                gas_used: 100_000,
                events: Vec::new(),
                output: Vec::new(),
            })
        }

        fn verify(
            &self,
            _state: &ContractState<'_>,
            _tx: &ContractTransaction,
            _result: &ExecutionResult,
        ) -> ContractResult<bool> {
            Ok(true)
        }
    }

    #[test]
    fn test_execute_transaction() {
        let processor = TransactionProcessor::default();
        let contract = MockContract { id: crate::crypto::Hash::ZERO };

        let kp = Keypair::generate();
        let sender = Address::from_public_key(kp.public_key());

        // Set up accounts
        let mut accounts = HashMap::new();
        accounts.insert(
            sender,
            AccountState::new(HclawAmount::from_hclaw(100)),
        );

        let mut storage = HashMap::new();

        // Create transaction
        let mut tx = ContractTransaction::new(
            contract.id(),
            kp.public_key().clone(),
            Vec::new(),
            1_000_000,
            HclawAmount::from_raw(1),
            1,
        );
        tx.signature = kp.sign(&tx.signing_bytes());

        let result = processor.execute_transaction(&contract, &tx, &mut accounts, &mut storage);
        assert!(result.is_ok());
    }
}
