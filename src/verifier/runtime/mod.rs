//! Multi-language verification runtime support.
//!
//! This module provides sandboxed execution environments for user-submitted
//! verification functions in Python, JavaScript/TypeScript, and eventually other languages.

mod python;
mod javascript;
pub mod capabilities;

pub use python::PythonRuntime;
pub use javascript::JavaScriptRuntime;
pub use capabilities::{ValidatorCapabilities, LanguageSupport, EnvironmentCheck};

use thiserror::Error;

/// Runtime execution errors
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Timeout exceeded during execution
    #[error("execution timeout exceeded ({0}ms)")]
    Timeout(u64),

    /// Memory limit exceeded
    #[error("memory limit exceeded ({0} bytes)")]
    MemoryLimitExceeded(usize),

    /// Execution failed with error
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    /// Invalid return type from verification function
    #[error("verification function must return boolean, got: {0}")]
    InvalidReturnType(String),

    /// Required function not found
    #[error("verification function '{0}' not found")]
    FunctionNotFound(String),

    /// Code hash mismatch
    #[error("code hash mismatch - possible tampering")]
    HashMismatch,

    /// Runtime not available
    #[error("runtime not available: {0}")]
    RuntimeNotAvailable(String),

    /// Network access attempted (security violation)
    #[error("network access is forbidden in verification functions")]
    NetworkAccessDenied,

    /// File system access attempted (security violation)
    #[error("file system access is forbidden in verification functions")]
    FileSystemAccessDenied,
}

/// Configuration for runtime sandboxing
#[derive(Clone, Debug)]
pub struct SandboxConfig {
    /// Maximum execution time in milliseconds
    pub timeout_ms: u64,
    /// Maximum memory usage in bytes
    pub max_memory_bytes: usize,
    /// Maximum stack size in bytes
    pub max_stack_bytes: usize,
    /// Allow network access (should always be false for production)
    pub allow_network: bool,
    /// Allow filesystem access (should always be false for production)
    pub allow_filesystem: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,           // 5 seconds
            max_memory_bytes: 100_000_000, // 100 MB
            max_stack_bytes: 8_000_000,  // 8 MB
            allow_network: false,
            allow_filesystem: false,
        }
    }
}

/// Trait for verification runtimes
pub trait VerificationRuntime: Send + Sync {
    /// Execute a verification function
    fn execute(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError>;

    /// Check if this runtime is available on the system
    fn is_available() -> bool;

    /// Get the language name
    fn language_name(&self) -> &'static str;

    /// Get resource usage after last execution
    fn last_execution_stats(&self) -> ExecutionStats;
}

/// Statistics from a verification execution
#[derive(Clone, Debug, Default)]
pub struct ExecutionStats {
    /// Time taken in milliseconds
    pub duration_ms: u64,
    /// Memory used in bytes
    pub memory_used: usize,
    /// Whether execution completed successfully
    pub success: bool,
}
