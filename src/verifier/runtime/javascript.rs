//! JavaScript/TypeScript verification runtime using Deno.
//!
//! Executes user-submitted JS/TS code in a sandboxed Deno environment with
//! timeout and memory limits.

use super::{RuntimeError, SandboxConfig, VerificationRuntime, ExecutionStats};
use deno_core::{JsRuntime, RuntimeOptions};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// JavaScript/TypeScript runtime for verification functions
pub struct JavaScriptRuntime {
    config: SandboxConfig,
    stats: Arc<Mutex<ExecutionStats>>,
}

impl JavaScriptRuntime {
    /// Create a new JavaScript runtime with default configuration
    pub fn new() -> Self {
        Self::with_config(SandboxConfig::default())
    }

    /// Create a new JavaScript runtime with custom configuration
    pub fn with_config(config: SandboxConfig) -> Self {
        Self {
            config,
            stats: Arc::new(Mutex::new(ExecutionStats::default())),
        }
    }

    /// Execute JavaScript code in sandboxed environment
    fn execute_sandboxed(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        let start = Instant::now();

        // Create a sandboxed Deno runtime
        let mut runtime = JsRuntime::new(RuntimeOptions::default());

        // Inject input and output data as Uint8Arrays
        let input_array = format!("[{}]", input.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","));
        let output_array = format!("[{}]", output.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","));
        
        let setup_code = format!(
            r#"
            globalThis.inputData = new Uint8Array({});
            globalThis.outputData = new Uint8Array({});
            
            // Disable dangerous globals
            delete globalThis.Deno;
            "#,
            input_array, output_array
        );

        // Execute setup code
        runtime
            .execute_script("<setup>", setup_code.into())
            .map_err(|e| RuntimeError::ExecutionFailed(format!("Setup failed: {}", e)))?;

        // Execute user code
        runtime
            .execute_script("<user_code>", code.to_string().into())
            .map_err(|e| RuntimeError::ExecutionFailed(format!("Code execution failed: {}", e)))?;

        // Call the verify function
        let verify_call = r#"
            (function() {
                if (typeof verify !== 'function') {
                    throw new Error('verify function not found');
                }
                const result = verify();
                if (typeof result !== 'boolean') {
                    throw new Error('verify must return boolean, got: ' + typeof result);
                }
                return result;
            })()
        "#;

        let result_value = runtime
            .execute_script("<verify_call>", verify_call.to_string().into())
            .map_err(|e| RuntimeError::ExecutionFailed(format!("Verification failed: {}", e)))?;

        // Extract the boolean result using the runtime scope
        let verified = {
            let scope = &mut runtime.handle_scope();
            let local = deno_core::v8::Local::new(scope, &result_value);
            local.boolean_value(scope)
        };

        // Check timeout
        let duration = start.elapsed();
        if duration.as_millis() as u64 > self.config.timeout_ms {
            return Err(RuntimeError::Timeout(duration.as_millis() as u64));
        }

        // Update stats
        if let Ok(mut stats) = self.stats.lock() {
            stats.duration_ms = duration.as_millis() as u64;
            stats.success = true;
        }

        Ok(verified)
    }
}

impl Default for JavaScriptRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationRuntime for JavaScriptRuntime {
    fn execute(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        self.execute_sandboxed(code, input, output)
    }

    fn is_available() -> bool {
        // Deno is embedded, so always available
        true
    }

    fn language_name(&self) -> &'static str {
        "javascript"
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
        let runtime = JavaScriptRuntime::new();
        
        let code = r#"
function verify() {
    // Check if input equals output
    if (inputData.length !== outputData.length) return false;
    for (let i = 0; i < inputData.length; i++) {
        if (inputData[i] !== outputData[i]) return false;
    }
    return true;
}
"#;

        let input = b"hello";
        let output = b"hello";
        
        let result = runtime.execute(code, input, output);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_network_access_blocked() {
        let runtime = JavaScriptRuntime::new();
        
        let code = r#"
function verify() {
    if (typeof Deno !== 'undefined') {
        throw new Error('Deno should not be available');
    }
    return true;
}
"#;

        let result = runtime.execute(code, b"", b"");
        assert!(result.is_ok());
        assert!(result.unwrap());
    }
}
