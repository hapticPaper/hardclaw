# Multi-Language Verification for HardClaw

## Overview

This document outlines the design and implementation plan for supporting user-submitted verification functions in Python, JavaScript, and TypeScript.

## Current Status

### ✅ Implemented
- **Hash Matching**: Fully functional for deterministic output verification
- **Framework**: `VerificationSpec` enum extended with `PythonScript` and `JavaScriptScript` variants
- **Validation**: Code hash verification to prevent tampering
- **Consensus**: 66% threshold properly implemented and tested

### ⚠️ Placeholder (Needs Implementation)
- **Python Execution**: Returns "not implemented" error
- **JavaScript Execution**: Returns "not implemented" error
- **WASM Execution**: Always returns true (placeholder)

## Architecture

### Verification Spec Types

```rust
pub enum VerificationSpec {
    // Existing - WORKS
    HashMatch { expected_hash: Hash },
    
    // Existing - PLACEHOLDER
    WasmVerifier { module_hash: Hash, entry_point: String },
    
    // NEW - PLACEHOLDER
    PythonScript { code_hash: Hash, code: String },
    
    // NEW - PLACEHOLDER
    JavaScriptScript { code_hash: Hash, code: String },
    
    // Partial - NEEDS INTEGRATION
    SchellingPoint { min_voters: u8, quality_threshold: u8 },
}
```

### Security Model

All user-submitted code must run in a sandboxed environment with:

1. **Resource Limits**
   - CPU timeout: 5 seconds default
   - Memory limit: 100 MB default
   - No network access
   - No filesystem access

2. **Code Integrity**
   - Hash verification before execution
   - Malicious code detection
   - Syntax validation

3. **Determinism**
   - Same input must always produce same output
   - No randomness or time-based behavior
   - No external state dependencies

## Implementation Plan

### Phase 1: Python Runtime (Priority 1)

#### Dependencies
```toml
[dependencies]
pyo3 = { version = "0.20", features = ["auto-initialize"] }
pyo3-asyncio = { version = "0.20", features = ["tokio-runtime"] }
```

#### API Contract

Verification scripts must define:
```python
def verify(input_bytes: bytes, output_bytes: bytes) -> bool:
    """
    Verify that output_bytes is a valid solution for input_bytes.
    
    Args:
        input_bytes: The job input data
        output_bytes: The solution output data
        
    Returns:
        True if solution is valid, False otherwise
    """
    pass
```

#### Example Usage

**Sort Numbers Job:**
```python
def verify(input_bytes: bytes, output_bytes: bytes) -> bool:
    """Verify output is sorted version of input."""
    input_list = list(input_bytes)
    output_list = list(output_bytes)
    expected = sorted(input_list)
    return output_list == expected
```

**JSON Validation Job:**
```python
import json

def verify(input_bytes: bytes, output_bytes: bytes) -> bool:
    """Verify output is valid JSON."""
    try:
        json.loads(output_bytes.decode('utf-8'))
        return True
    except:
        return False
```

#### Implementation Structure

```rust
// src/verifier/runtime/python.rs

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyModule};
use std::time::Duration;

pub struct PythonRuntime {
    timeout: Duration,
    max_memory_bytes: usize,
}

impl PythonRuntime {
    pub fn new(timeout: Duration, max_memory_bytes: usize) -> Self {
        Self {
            timeout,
            max_memory_bytes,
        }
    }

    pub fn execute_verifier(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        Python::with_gil(|py| {
            // Set resource limits
            self.set_limits(py)?;
            
            // Compile and execute code
            let module = PyModule::from_code(py, code, "verifier", "verifier")?;
            
            // Get verify function
            let verify = module.getattr("verify")?;
            
            // Convert bytes to Python bytes
            let input_bytes = PyBytes::new(py, input);
            let output_bytes = PyBytes::new(py, output);
            
            // Call with timeout
            let result = self.call_with_timeout(py, verify, (input_bytes, output_bytes))?;
            
            // Extract boolean result
            Ok(result.extract::<bool>()?)
        })
    }

    fn set_limits(&self, py: Python) -> PyResult<()> {
        // Set memory limit
        let resource = py.import("resource")?;
        resource.call_method1(
            "setrlimit",
            (
                resource.getattr("RLIMIT_AS")?,
                (self.max_memory_bytes, self.max_memory_bytes),
            ),
        )?;
        
        Ok(())
    }

    fn call_with_timeout(
        &self,
        py: Python,
        func: &PyAny,
        args: impl IntoPy<Py<PyTuple>>,
    ) -> PyResult<PyObject> {
        // Use signal.alarm for timeout
        let signal = py.import("signal")?;
        signal.call_method1("alarm", (self.timeout.as_secs(),))?;
        
        let result = func.call1(args);
        
        // Cancel alarm
        signal.call_method1("alarm", (0,))?;
        
        result
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Python execution error: {0}")]
    ExecutionError(#[from] PyErr),
    
    #[error("Timeout exceeded")]
    Timeout,
    
    #[error("Memory limit exceeded")]
    MemoryLimitExceeded,
    
    #[error("Invalid return type")]
    InvalidReturnType,
}
```

### Phase 2: JavaScript/TypeScript Runtime (Priority 2)

#### Dependencies
```toml
[dependencies]
deno_core = "0.258"
deno_runtime = "0.140"
```

#### API Contract

Verification scripts must define:
```typescript
function verify(input: Uint8Array, output: Uint8Array): boolean {
    // Verify that output is valid solution for input
    return true; // or false
}
```

#### Example Usage

**Hash Verification:**
```typescript
function verify(input: Uint8Array, output: Uint8Array): boolean {
    const hash = crypto.subtle.digest('SHA-256', output);
    const expectedHash = new Uint8Array(input);
    return arrayEqual(hash, expectedHash);
}

function arrayEqual(a: Uint8Array, b: Uint8Array): boolean {
    return a.length === b.length && a.every((v, i) => v === b[i]);
}
```

**Image Validation:**
```typescript
function verify(input: Uint8Array, output: Uint8Array): boolean {
    // Check output is valid PNG
    const signature = new Uint8Array([0x89, 0x50, 0x4E, 0x47]);
    return output.slice(0, 4).every((v, i) => v === signature[i]);
}
```

#### Implementation Structure

```rust
// src/verifier/runtime/javascript.rs

use deno_core::{JsRuntime, RuntimeOptions};
use std::time::Duration;

pub struct JavaScriptRuntime {
    timeout: Duration,
    max_memory_bytes: usize,
}

impl JavaScriptRuntime {
    pub fn new(timeout: Duration, max_memory_bytes: usize) -> Self {
        Self {
            timeout,
            max_memory_bytes,
        }
    }

    pub fn execute_verifier(
        &self,
        code: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            // Disable all extensions for security
            extensions: vec![],
            // Set memory limits
            max_heap_size: Some(self.max_memory_bytes),
            ..Default::default()
        });

        // Inject input/output as global variables
        let setup = format!(
            r#"
            globalThis.INPUT = new Uint8Array({:?});
            globalThis.OUTPUT = new Uint8Array({:?});
            "#,
            input, output
        );
        runtime.execute_script("<setup>", &setup)?;

        // Execute user code
        runtime.execute_script("<verifier>", code)?;

        // Call verify function with timeout
        let result_future = runtime.execute_script(
            "<call>",
            "verify(globalThis.INPUT, globalThis.OUTPUT)",
        )?;

        // Extract boolean result
        let result = tokio::time::timeout(
            self.timeout,
            async move { runtime.resolve_value(result_future).await },
        )
        .await
        .map_err(|_| RuntimeError::Timeout)??;

        Ok(result.to_boolean())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("JavaScript execution error: {0}")]
    ExecutionError(String),
    
    #[error("Timeout exceeded")]
    Timeout,
    
    #[error("Memory limit exceeded")]
    MemoryLimitExceeded,
    
    #[error("Invalid return type")]
    InvalidReturnType,
}
```

### Phase 3: WASM Runtime (Priority 3)

#### Dependencies
```toml
[dependencies]
wasmer = "4.2"
wasmer-compiler-cranelift = "4.2"
```

#### Implementation Structure

```rust
// src/verifier/runtime/wasm.rs

use wasmer::{Store, Module, Instance, imports, Function, FunctionEnv};
use std::time::Duration;

pub struct WasmRuntime {
    store: Store,
    timeout: Duration,
    max_memory_pages: u32,
}

impl WasmRuntime {
    pub fn new(timeout: Duration, max_memory_pages: u32) -> Self {
        let mut store = Store::default();
        
        Self {
            store,
            timeout,
            max_memory_pages,
        }
    }

    pub fn execute_verifier(
        &mut self,
        module_bytes: &[u8],
        entry_point: &str,
        input: &[u8],
        output: &[u8],
    ) -> Result<bool, RuntimeError> {
        // Compile module
        let module = Module::new(&self.store, module_bytes)?;
        
        // Set up imports (memory, functions)
        let import_object = imports! {};
        
        // Instantiate
        let instance = Instance::new(&mut self.store, &module, &import_object)?;
        
        // Get memory and write input/output
        let memory = instance.exports.get_memory("memory")?;
        // ... write data to memory ...
        
        // Get verify function
        let verify = instance.exports.get_function(entry_point)?;
        
        // Call with timeout
        let result = verify.call(&mut self.store, &[])?;
        
        // Extract boolean
        Ok(result[0].unwrap_i32() != 0)
    }
}
```

## Testing Strategy

### Unit Tests

Each runtime must pass these tests:

1. **Valid Verification**
   ```rust
   #[test]
   fn test_python_valid_solution() {
       let code = r#"
   def verify(input_bytes, output_bytes):
       return output_bytes == b"expected"
   "#;
       let runtime = PythonRuntime::new(Duration::from_secs(5), 100_000_000);
       assert!(runtime.execute_verifier(code, b"input", b"expected").unwrap());
   }
   ```

2. **Invalid Verification**
   ```rust
   #[test]
   fn test_python_invalid_solution() {
       let code = r#"
   def verify(input_bytes, output_bytes):
       return output_bytes == b"expected"
   "#;
       let runtime = PythonRuntime::new(Duration::from_secs(5), 100_000_000);
       assert!(!runtime.execute_verifier(code, b"input", b"wrong").unwrap());
   }
   ```

3. **Timeout Protection**
   ```rust
   #[test]
   fn test_python_timeout() {
       let code = r#"
   def verify(input_bytes, output_bytes):
       while True:  # Infinite loop
           pass
   "#;
       let runtime = PythonRuntime::new(Duration::from_secs(1), 100_000_000);
       let result = runtime.execute_verifier(code, b"input", b"output");
       assert!(matches!(result, Err(RuntimeError::Timeout)));
   }
   ```

4. **Memory Limit**
   ```rust
   #[test]
   fn test_python_memory_limit() {
       let code = r#"
   def verify(input_bytes, output_bytes):
       data = bytearray(200 * 1024 * 1024)  # Allocate 200 MB
       return True
   "#;
       let runtime = PythonRuntime::new(Duration::from_secs(5), 100_000_000);
       let result = runtime.execute_verifier(code, b"input", b"output");
       assert!(matches!(result, Err(RuntimeError::MemoryLimitExceeded)));
   }
   ```

5. **Code Injection**
   ```rust
   #[test]
   fn test_python_no_network_access() {
       let code = r#"
   import urllib.request
   def verify(input_bytes, output_bytes):
       urllib.request.urlopen('http://evil.com')
       return True
   "#;
       let runtime = PythonRuntime::new(Duration::from_secs(5), 100_000_000);
       let result = runtime.execute_verifier(code, b"input", b"output");
       assert!(result.is_err());
   }
   ```

### Integration Tests

```rust
#[test]
fn test_end_to_end_python_verification() {
    // Create a job with Python verifier
    let verifier_code = r#"
def verify(input_bytes, output_bytes):
    # Verify output is sorted version of input
    input_list = list(input_bytes)
    output_list = list(output_bytes)
    return output_list == sorted(input_list)
"#;
    
    let code_hash = hash_data(verifier_code.as_bytes());
    
    let mut job = JobPacket::new(
        JobType::Deterministic,
        requester_pubkey,
        vec![5, 2, 8, 1, 9],
        "Sort these numbers".to_string(),
        HclawAmount::from_hclaw(10),
        HclawAmount::from_hclaw(1),
        VerificationSpec::PythonScript {
            code_hash,
            code: verifier_code.to_string(),
        },
        3600,
    );
    
    // Submit solution
    let solution = SolutionCandidate::new(
        job.id,
        solver_pubkey,
        vec![1, 2, 5, 8, 9],  // Sorted
    );
    
    // Verify
    let mut verifier = Verifier::new(keypair, VerifierConfig::default());
    let (result, _) = verifier.process_solution(&job, &solution).unwrap();
    
    assert!(result.passed);
}
```

## Security Considerations

### Attack Vectors

1. **Infinite Loops**: Mitigated by timeout
2. **Memory Exhaustion**: Mitigated by memory limits
3. **Network Access**: Disabled in sandbox
4. **File System Access**: Disabled in sandbox
5. **Code Injection**: Hash verification prevents tampering
6. **Non-Determinism**: Script guidelines enforce deterministic behavior

### Audit Checklist

- [ ] Timeout enforcement tested
- [ ] Memory limits tested
- [ ] Network access blocked
- [ ] Filesystem access blocked
- [ ] Process isolation verified
- [ ] Code hash validation works
- [ ] No privilege escalation possible
- [ ] Resource cleanup on error

## Performance Benchmarks

Target performance:

- **Hash verification**: < 1ms
- **Python verification**: < 100ms (simple)
- **JavaScript verification**: < 50ms (simple)
- **WASM verification**: < 10ms (simple)

Throughput goals:

- 1000+ verifications/second per verifier node
- Scale to 10,000+ concurrent jobs

## Migration Timeline

### Week 1-2: Python Runtime
- [ ] Integrate PyO3
- [ ] Implement sandboxing
- [ ] Add timeout/memory limits
- [ ] Unit tests
- [ ] Integration tests

### Week 3: JavaScript Runtime
- [ ] Integrate Deno
- [ ] Implement sandboxing
- [ ] Add timeout/memory limits
- [ ] Unit tests
- [ ] Integration tests

### Week 4: WASM Runtime
- [ ] Integrate Wasmer
- [ ] Implement gas metering
- [ ] Module storage
- [ ] Unit tests

### Week 5: Security Hardening
- [ ] Security audit
- [ ] Penetration testing
- [ ] Performance optimization

### Week 6: Documentation & Release
- [ ] API documentation
- [ ] Example verifiers
- [ ] Migration guide
- [ ] Production deployment

## Usage Examples

### Python: Image Classification
```python
def verify(input_bytes: bytes, output_bytes: bytes) -> bool:
    """Verify image classification result."""
    import json
    
    # Parse classification result
    result = json.loads(output_bytes.decode('utf-8'))
    
    # Check required fields
    if 'label' not in result or 'confidence' not in result:
        return False
    
    # Verify confidence is reasonable
    confidence = float(result['confidence'])
    return 0.0 <= confidence <= 1.0
```

### JavaScript: Data Transformation
```typescript
function verify(input: Uint8Array, output: Uint8Array): boolean {
    // Verify CSV to JSON transformation
    const inputStr = new TextDecoder().decode(input);
    const outputStr = new TextDecoder().decode(output);
    
    try {
        const json = JSON.parse(outputStr);
        const lines = inputStr.split('\n');
        
        // Verify row count matches
        return json.length === lines.length - 1;
    } catch {
        return false;
    }
}
```

### WASM: Math Computation
```rust
// Compiled to WASM
#[no_mangle]
pub extern "C" fn verify(input_ptr: *const u8, input_len: usize,
                         output_ptr: *const u8, output_len: usize) -> i32 {
    // Verify mathematical computation result
    let input = unsafe { std::slice::from_raw_parts(input_ptr, input_len) };
    let output = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    
    // Parse numbers and verify
    let expected = compute_result(input);
    (output == expected) as i32
}
```

## Conclusion

This multi-language verification system enables HardClaw to support a wide range of tasks while maintaining security and consensus integrity. The phased implementation approach allows for incremental deployment and testing.
