//! Contract Loading Infrastructure
//!
//! Handles loading contracts from bytecode, enforcing versioning,
//! and routing to the appropriate runtime (WASM vs Native).

use crate::contracts::wasm::WasmContract;
use crate::contracts::Contract;
use crate::contracts::ContractError;
use crate::contracts::ContractResult;
use crate::types::Id;

/// A trait for loading contracts from bytecode
pub trait ContractLoader: Send + Sync {
    /// Try to load a contract from bytecode
    fn load(&self, id: Id, code: &[u8]) -> ContractResult<Box<dyn Contract>>;
}

/// The main contract loader that delegates to specific runtimes
pub struct UniversalLoader {
    native_loader: NativeLoader,
    wasm_loader: WasmLoader,
}

impl UniversalLoader {
    /// Create a new universal loader with native and WASM runtimes.
    pub fn new() -> Self {
        Self {
            native_loader: NativeLoader {},
            wasm_loader: WasmLoader {},
        }
    }
}

impl Default for UniversalLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractLoader for UniversalLoader {
    fn load(&self, id: Id, code: &[u8]) -> ContractResult<Box<dyn Contract>> {
        // Check for native marker
        if code.starts_with(b"native:") {
            return self.native_loader.load(id, code);
        }

        // Check for WASM magic bytes (\0asm)
        if code.starts_with(&[0x00, 0x61, 0x73, 0x6d]) {
            return self.wasm_loader.load(id, code);
        }

        Err(ContractError::ExecutionFailed(
            "Unknown contract format".to_string(),
        ))
    }
}

/// Loads native implementations (Genesis contracts)
struct NativeLoader;

impl ContractLoader for NativeLoader {
    fn load(&self, _id: Id, code: &[u8]) -> ContractResult<Box<dyn Contract>> {
        let marker = String::from_utf8_lossy(code);
        match marker.trim() {
            "native:genesis_bounty_v1" => {
                // Return a new instance of the GenesisBountyContract
                // Note: The start_time here is just a default; INIT logic will set real values
                Ok(Box::new(
                    crate::contracts::genesis_bounty::GenesisBountyContract::new(0),
                ))
            }
            _ => Err(ContractError::ExecutionFailed(format!(
                "Unknown native contract: {}",
                marker
            ))),
        }
    }
}

/// Loads WASM contracts
struct WasmLoader;

impl ContractLoader for WasmLoader {
    fn load(&self, id: Id, code: &[u8]) -> ContractResult<Box<dyn Contract>> {
        Ok(Box::new(WasmContract::new(id, code.to_vec())))
    }
}
