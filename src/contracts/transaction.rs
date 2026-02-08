//! Contract transaction types.
//!
//! Transactions are proposals to execute contract logic.
//! They contain the contract ID, input data, and sender information.

use serde::{Deserialize, Serialize};

use crate::crypto::{Hash, PublicKey, Signature};
use crate::types::{Address, HclawAmount, Id, Timestamp};

/// A transaction that executes a smart contract
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractTransaction {
    /// Transaction ID (hash of contents)
    pub id: Id,
    /// Contract to execute
    pub contract_id: Id,
    /// Sender's public key
    pub sender: PublicKey,
    /// Sender's address
    pub sender_address: Address,
    /// Input data for contract execution
    pub input: Vec<u8>,
    /// Maximum gas willing to pay
    pub gas_limit: u64,
    /// Gas price (HCLAW per unit)
    pub gas_price: HclawAmount,
    /// Nonce (for ordering transactions from same sender)
    pub nonce: u64,
    /// When transaction was created
    pub timestamp: Timestamp,
    /// Sender's signature
    pub signature: Signature,
}

impl ContractTransaction {
    /// Create new contract transaction (unsigned)
    #[must_use]
    pub fn new(
        contract_id: Id,
        sender: PublicKey,
        input: Vec<u8>,
        gas_limit: u64,
        gas_price: HclawAmount,
        nonce: u64,
    ) -> Self {
        let sender_address = Address::from_public_key(&sender);
        let timestamp = crate::types::now_millis();

        let mut tx = Self {
            id: Hash::ZERO,
            contract_id,
            sender,
            sender_address,
            input,
            gas_limit,
            gas_price,
            nonce,
            timestamp,
            signature: Signature::placeholder(),
        };

        tx.id = tx.compute_id();
        tx
    }

    /// Compute transaction ID
    #[must_use]
    pub fn compute_id(&self) -> Id {
        use crate::crypto::hash_data;

        let mut data = Vec::new();
        data.extend_from_slice(self.contract_id.as_bytes());
        data.extend_from_slice(self.sender.as_bytes());
        data.extend_from_slice(&self.input);
        data.extend_from_slice(&self.gas_limit.to_le_bytes());
        data.extend_from_slice(&self.gas_price.raw().to_le_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());

        hash_data(&data)
    }

    /// Get bytes to sign
    #[must_use]
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.id.as_bytes());
        data.extend_from_slice(self.contract_id.as_bytes());
        data.extend_from_slice(self.sender.as_bytes());
        data.extend_from_slice(&self.input);
        data.extend_from_slice(&self.gas_limit.to_le_bytes());
        data.extend_from_slice(&self.gas_price.raw().to_le_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        data
    }

    /// Verify transaction signature
    ///
    /// # Errors
    /// Returns error if signature is invalid
    pub fn verify_signature(&self) -> Result<(), crate::crypto::CryptoError> {
        crate::crypto::verify(&self.sender, &self.signing_bytes(), &self.signature)
    }

    /// Maximum fee (`gas_limit` * `gas_price`)
    #[must_use]
    pub fn max_fee(&self) -> HclawAmount {
        HclawAmount::from_raw(self.gas_price.raw() * self.gas_limit as u128)
    }
}

/// Types of contract transactions
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TransactionKind {
    /// Execute a contract
    Execute(ContractTransaction),
    /// Deploy a new contract
    Deploy {
        /// Contract bytecode/definition
        code: Vec<u8>,
        /// Initialization parameters
        init_data: Vec<u8>,
        /// Deployer
        deployer: Address,
    },
    /// Upgrade an existing contract
    Upgrade {
        /// Contract ID to upgrade
        contract_id: Id,
        /// New code
        new_code: Vec<u8>,
        /// Upgrader address
        upgrader: Address,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    #[test]
    fn test_transaction_creation() {
        let kp = Keypair::generate();
        let contract_id = Hash::ZERO;

        let tx = ContractTransaction::new(
            contract_id,
            kp.public_key().clone(),
            b"test input".to_vec(),
            1_000_000,
            HclawAmount::from_raw(1),
            1,
        );

        assert_eq!(tx.contract_id, contract_id);
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.gas_limit, 1_000_000);
    }

    #[test]
    fn test_transaction_id_deterministic() {
        let kp = Keypair::generate();
        let contract_id = Hash::ZERO;

        let tx1 = ContractTransaction::new(
            contract_id,
            kp.public_key().clone(),
            b"same input".to_vec(),
            1_000_000,
            HclawAmount::from_raw(1),
            1,
        );

        let computed_id = tx1.compute_id();
        assert_eq!(tx1.id, computed_id);
    }

    #[test]
    fn test_max_fee() {
        let kp = Keypair::generate();
        let tx = ContractTransaction::new(
            Hash::ZERO,
            kp.public_key().clone(),
            b"test".to_vec(),
            1_000,
            HclawAmount::from_raw(100),
            1,
        );

        assert_eq!(tx.max_fee().raw(), 100_000);
    }
}
