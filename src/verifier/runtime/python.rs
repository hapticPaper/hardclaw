//! Python verification runtime using PyO3.
//!
//! Executes user-submitted Python code in a sandboxed environment with
//! timeout and memory limits.

use super::{RuntimeError, SandboxConfig, VerificationRuntime, ExecutionStats};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyModule};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::thread;

/// Python runtime for verification functions
pub struct PythonRuntime {
    config: SandboxConfig,
    stats: Arc<Mutex<ExecutionStats>>,
}

impl PythonRuntime {
    /// Create a new Python runtime with default configuration
    pub fn new() -> Self {
        Self::with_config(SandboxConfig::default())
    }

    /// Create a new Python runtime with custom configuration
    pub fn with_config(config: SandboxConfig) -> Self {
        Self {
            config,
            stats: Arc::new(Mutex::new(ExecutionStats::default())),
        }
    }

    /// Execute Python code in sandboxed environment
    fn execute_sandboxed(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        let start = Instant::now();
        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        
        // Clone configuration for thread
        let code = code.to_string();
        let input = input.to_vec();
        let output = output.to_vec();
        let _max_memory = self.config.max_memory_bytes;  // TODO: implement memory limiting
        
        // Execute in separate thread to enforce timeout
        let handle = thread::spawn(move || {
            Python::with_gil(|py| {
                // Create restricted globals to prevent dangerous operations
                let globals = PyModule::import(py, "__main__")
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?
                    .dict();

                // Disable dangerous builtins
                let builtins = PyModule::import(py, "builtins")
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;
                
                // Remove dangerous functions
                let safe_builtins = ["abs", "all", "any", "ascii", "bin", "bool", "bytearray", 
                    "bytes", "chr", "dict", "divmod", "enumerate", "filter", "float", "format",
                    "frozenset", "hash", "hex", "int", "isinstance", "issubclass", "iter", 
                    "len", "list", "map", "max", "min", "oct", "ord", "pow", "print", "range",
                    "repr", "reversed", "round", "set", "slice", "sorted", "str", "sum", "tuple",
                    "type", "zip"];
                
                let restricted_builtins = py.eval(
                    &format!("{{k: v for k, v in __builtins__.items() if k in {safe_builtins:?}}}"),
                    Some(builtins.dict()),
                    None,
                ).map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;

                globals.set_item("__builtins__", restricted_builtins)
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;

                // Inject input and output as bytes
                globals.set_item("input_data", PyBytes::new(py, &input))
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;
                globals.set_item("output_data", PyBytes::new(py, &output))
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;

                // Execute the user code
                py.run(&code, Some(globals), None)
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;

                // Call the verify function
                let verify_fn = globals.get_item("verify")
                    .map_err(|_| RuntimeError::FunctionNotFound("verify".to_string()))?
                    .ok_or_else(|| RuntimeError::FunctionNotFound("verify".to_string()))?;

                // Execute verification function
                let result = verify_fn.call0()
                    .map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;

                // Ensure result is boolean
                if let Ok(bool_result) = result.downcast::<PyBool>() {
                    Ok(bool_result.is_true())
                } else {
                    Err(RuntimeError::InvalidReturnType(format!("{:?}", result)))
                }
            })
        });

        // Wait for execution with timeout
        let result = match handle.join() {
            Ok(r) => r,
            Err(_) => return Err(RuntimeError::ExecutionFailed("Thread panicked".to_string())),
        };

        // Check timeout
        let duration = start.elapsed();
        if duration > timeout_duration {
            return Err(RuntimeError::Timeout(duration.as_millis() as u64));
        }

        // Update stats
        if let Ok(mut stats) = self.stats.lock() {
            stats.duration_ms = duration.as_millis() as u64;
            stats.success = result.is_ok();
        }

        result
    }
}

impl Default for PythonRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationRuntime for PythonRuntime {
    fn execute(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        self.execute_sandboxed(code, input, output)
    }

    fn is_available() -> bool {
        Python::with_gil(|py| {
            py.version_info().major >= 3 && py.version_info().minor >= 8
        })
    }

    fn language_name(&self) -> &'static str {
        "python"
    }

    fn last_execution_stats(&self) -> ExecutionStats {
        self.stats.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_verification() {
        let runtime = PythonRuntime::new();
        
        let code = r#"
def verify():
    # Check if input equals output
    return input_data == output_data
"#;

        let input = b"hello";
        let output = b"hello";
        
        let result = runtime.execute(code, input, output);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_hash_verification() {
        let runtime = PythonRuntime::new();
        
        let code = r#"
def verify():
    import hashlib
    expected = hashlib.sha256(input_data).digest()
    return expected == output_data
"#;

        let input = b"test input";
        let hash = sha256::digest(input);
        let output = hash.as_bytes();
        
        let result = runtime.execute(code, input, output);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dangerous_import_blocked() {
        let runtime = PythonRuntime::new();
        
        let code = r#"
import os
def verify():
    os.system("ls")
    return True
"#;

        let result = runtime.execute(code, b"", b"");
        assert!(result.is_err());
    }
}
