//! Validator capabilities and environment checking.
//!
//! This module manages what languages a validator can execute and provides
//! environment setup validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Get the validator's data directory
fn validator_data_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());

    PathBuf::from(home).join(".hardclaw")
}

/// Configuration for validator's Python environment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonSandboxConfig {
    /// Path to the standalone Python installation
    pub python_dir: PathBuf,
    /// Python binary path
    pub python_bin: PathBuf,
}

/// Configuration for validator's AI models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIModelConfig {
    /// Directory containing Ollama models
    pub ollama_models_dir: PathBuf,
    /// Models downloaded and available locally
    pub downloaded_models: Vec<String>,
}

/// Languages supported by the verification system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LanguageSupport {
    /// Python runtime support
    Python,
    /// JavaScript runtime support
    JavaScript,
    /// TypeScript runtime support
    TypeScript,
    /// WebAssembly runtime support
    Wasm,
}

impl LanguageSupport {
    /// Get all supported languages
    pub fn all() -> Vec<Self> {
        vec![Self::Python, Self::JavaScript, Self::TypeScript, Self::Wasm]
    }

    /// Get the language name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Wasm => "wasm",
        }
    }

    /// Get the display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Wasm => "WebAssembly",
        }
    }
}

/// Environment check results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentCheck {
    /// Language being checked
    pub language: LanguageSupport,
    /// Whether the language runtime is available
    pub available: bool,
    /// Version information if available
    pub version: Option<String>,
    /// Any warnings or issues
    pub warnings: Vec<String>,
    /// Setup instructions if not available
    pub setup_instructions: Option<String>,
}

/// AI model check for safety review
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIModelCheck {
    /// Whether AI review is available
    pub available: bool,
    /// Configured models
    pub models: Vec<String>,
    /// Warnings
    pub warnings: Vec<String>,
    /// Setup instructions
    pub setup_instructions: Option<String>,
}

impl AIModelCheck {
    /// Detect existing Ollama installation and models (read-only, no installs)
    pub fn detect() -> Self {
        let mut warnings = Vec::new();
        let mut setup_instructions = None;

        // Check if Ollama is installed
        let ollama_check = Command::new("ollama").arg("--version").output();
        let ollama_installed = matches!(ollama_check, Ok(output) if output.status.success());

        if !ollama_installed {
            setup_instructions = Some(
                "Install Ollama:\n  https://ollama.com/download\n  Then run: ollama pull llama3.2"
                    .to_string(),
            );
            return Self {
                available: false,
                models: Vec::new(),
                warnings,
                setup_instructions,
            };
        }

        // Check available models (don't download anything)
        let models_output = Command::new("ollama").arg("list").output();

        let models = match models_output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout
                    .lines()
                    .skip(1) // Skip header
                    .filter_map(|line| line.split_whitespace().next())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            }
            _ => {
                warnings.push("Could not list Ollama models (is service running?)".to_string());
                Vec::new()
            }
        };

        if models.is_empty() {
            setup_instructions = Some("No models found. Run: ollama pull llama3.2".to_string());
        }

        Self {
            available: !models.is_empty(),
            models,
            warnings,
            setup_instructions,
        }
    }

    /// Setup AI models - install Ollama if needed, download specified models
    pub fn setup(models_to_download: &[&str]) -> Self {
        let mut warnings = Vec::new();
        let mut setup_instructions = None;

        let data_dir = validator_data_dir();
        let config_path = data_dir.join("ai-models-config.json");

        // Step 1: Install Ollama if not present
        let ollama_check = Command::new("ollama").arg("--version").output();
        let ollama_ok = matches!(ollama_check, Ok(output) if output.status.success());

        if !ollama_ok {
            eprintln!("📦 Installing Ollama...");

            #[cfg(target_os = "macos")]
            let install_result = Command::new("sh")
                .arg("-c")
                .arg("brew install ollama && brew services start ollama")
                .status();

            #[cfg(target_os = "linux")]
            let install_result = Command::new("sh")
                .arg("-c")
                .arg("curl -fsSL https://ollama.com/install.sh | sh && sudo systemctl enable ollama && sudo systemctl start ollama")
                .status();

            #[cfg(target_os = "windows")]
            let install_result = {
                // winget returns immediately while the installer runs async in another window.
                // Use --wait to block until the install actually completes.
                Command::new("winget")
                    .args(["install", "Ollama.Ollama", "--wait"])
                    .status()
            };

            #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
            let install_result: Result<std::process::ExitStatus, std::io::Error> =
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Auto-install not supported on this OS",
                ));

            match install_result {
                Ok(status) if status.success() => {
                    eprintln!("✅ Ollama installed");

                    // On Windows, even with --wait, the PATH may not be updated in this
                    // process. Poll for the binary to become reachable.
                    #[cfg(target_os = "windows")]
                    {
                        eprintln!("⏳ Waiting for Ollama to become available...");
                        let mut found = false;
                        for _ in 0..30 {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            if Command::new("ollama")
                                .arg("--version")
                                .output()
                                .map(|o| o.status.success())
                                .unwrap_or(false)
                            {
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            warnings.push(
                                "Ollama installed but not yet on PATH — you may need to restart your terminal".to_string(),
                            );
                        }
                    }

                    #[cfg(not(target_os = "windows"))]
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }
                _ => {
                    setup_instructions =
                        Some("Install Ollama manually:\n  https://ollama.com/download".to_string());
                    return Self {
                        available: false,
                        models: Vec::new(),
                        warnings: vec!["Ollama installation failed".to_string()],
                        setup_instructions,
                    };
                }
            }
        }

        // Step 2: Ensure Ollama service is running
        #[cfg(target_os = "linux")]
        {
            Command::new("sudo")
                .args(["systemctl", "start", "ollama"])
                .status()
                .ok();
        }

        #[cfg(target_os = "macos")]
        {
            Command::new("brew")
                .args(["services", "start", "ollama"])
                .status()
                .ok();
        }

        #[cfg(target_os = "windows")]
        {
            // Start Ollama serve in background (Windows has no systemd/launchd)
            Command::new("cmd")
                .args(["/C", "start", "/B", "ollama", "serve"])
                .spawn()
                .ok();
        }

        std::thread::sleep(std::time::Duration::from_secs(2));

        // Step 3: Get current models
        let models_output = Command::new("ollama").arg("list").output();
        let mut models: Vec<String> = match models_output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout
                    .lines()
                    .skip(1)
                    .filter_map(|line| line.split_whitespace().next())
                    .map(ToString::to_string)
                    .collect()
            }
            _ => Vec::new(),
        };

        // Step 4: Download only requested models that aren't already present
        for model in models_to_download {
            if models.iter().any(|m| m.starts_with(model)) {
                eprintln!("  ✓ {model} (already installed)");
            } else {
                eprintln!("📥 Downloading {model} (this may take several minutes)...");

                let pull_result = Command::new("ollama").args(["pull", model]).status();

                match pull_result {
                    Ok(status) if status.success() => {
                        eprintln!("  ✓ {model}");
                        models.push(model.to_string());
                    }
                    _ => {
                        warnings.push(format!("Failed to download {model}"));
                    }
                }
            }
        }

        // Step 5: Save config
        fs::create_dir_all(&data_dir).ok();

        let ollama_models_dir = std::env::var("OLLAMA_MODELS").map_or_else(
            |_| {
                #[cfg(target_os = "macos")]
                {
                    PathBuf::from("/usr/local/share/ollama/models")
                }
                #[cfg(target_os = "linux")]
                {
                    PathBuf::from("/usr/share/ollama/.ollama/models")
                }
                #[cfg(target_os = "windows")]
                {
                    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default())
                        .join(".ollama")
                        .join("models")
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
                {
                    PathBuf::from(".ollama/models")
                }
            },
            PathBuf::from,
        );

        let config = AIModelConfig {
            ollama_models_dir,
            downloaded_models: models.clone(),
        };

        if let Ok(json) = serde_json::to_string_pretty(&config) {
            fs::write(&config_path, json).ok();
            eprintln!("💾 Saved AI config to {}", config_path.display());
        }

        Self {
            available: !models.is_empty(),
            models,
            warnings,
            setup_instructions,
        }
    }

    /// Legacy method - detects without installing (renamed from check)
    pub fn check() -> Self {
        Self::detect()
    }

    /// Check if suitable model is available for code review
    pub fn has_code_model(&self) -> bool {
        self.models
            .iter()
            .any(|m| m.contains("llama") || m.contains("code") || m.contains("qwen"))
    }
}

impl EnvironmentCheck {
    /// Get the standalone Python download URL for this platform
    #[allow(clippy::unnecessary_wraps)] // Option is needed for unsupported platforms
    fn python_standalone_url() -> Option<&'static str> {
        // Using python-build-standalone releases (portable, no install needed)
        // https://github.com/indygreg/python-build-standalone
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Some("https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-aarch64-apple-darwin-install_only.tar.gz")
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            Some("https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-x86_64-apple-darwin-install_only.tar.gz")
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            Some("https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-x86_64-unknown-linux-gnu-install_only.tar.gz")
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            Some("https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-aarch64-unknown-linux-gnu-install_only.tar.gz")
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Some("https://github.com/indygreg/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-x86_64-pc-windows-msvc-install_only.tar.gz")
        }
        #[cfg(not(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64"),
        )))]
        {
            None
        }
    }

    /// Get path to our standalone Python installation
    fn standalone_python_dir() -> PathBuf {
        validator_data_dir().join("python")
    }

    /// Get path to the Python binary in our standalone installation
    fn standalone_python_bin() -> PathBuf {
        let python_dir = Self::standalone_python_dir();
        #[cfg(target_os = "windows")]
        {
            python_dir.join("python").join("python.exe")
        }
        #[cfg(not(target_os = "windows"))]
        {
            python_dir.join("python").join("bin").join("python3")
        }
    }

    /// Detect Python environment (checks if standalone Python is installed)
    pub fn detect_python() -> Self {
        let python_bin = Self::standalone_python_bin();

        if python_bin.exists() {
            // Test that it actually works
            let version_check = Command::new(&python_bin).arg("--version").output();

            match version_check {
                Ok(output) if output.status.success() => {
                    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    return Self {
                        language: LanguageSupport::Python,
                        available: true,
                        version: Some(format!("{version} (standalone)")),
                        warnings: Vec::new(),
                        setup_instructions: None,
                    };
                }
                _ => {}
            }
        }

        // Not installed
        Self {
            language: LanguageSupport::Python,
            available: false,
            version: None,
            warnings: Vec::new(),
            setup_instructions: Some(
                "Select 'Python' in environment setup to download standalone Python".to_string(),
            ),
        }
    }

    /// Setup Python - downloads and installs standalone Python distribution
    pub fn setup_python() -> Self {
        let data_dir = validator_data_dir();
        let python_dir = Self::standalone_python_dir();
        let python_bin = Self::standalone_python_bin();
        let config_path = data_dir.join("python-config.json");

        // Already installed?
        if python_bin.exists() {
            let check = Command::new(&python_bin).arg("--version").output();
            if let Ok(output) = check {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    return Self {
                        language: LanguageSupport::Python,
                        available: true,
                        version: Some(format!("{version} (standalone)")),
                        warnings: vec!["Already installed".to_string()],
                        setup_instructions: None,
                    };
                }
            }
        }

        let Some(url) = Self::python_standalone_url() else {
            return Self {
                language: LanguageSupport::Python,
                available: false,
                version: None,
                warnings: vec!["Platform not supported for auto-install".to_string()],
                setup_instructions: Some(
                    "Install Python 3.12+ manually from https://python.org".to_string(),
                ),
            };
        };

        eprintln!("📦 Downloading standalone Python 3.12...");
        eprintln!("   This is a one-time ~30MB download");

        // Create directory
        fs::create_dir_all(&python_dir).ok();

        let archive_path = python_dir.join("python.tar.gz");

        // Download using curl (available on all platforms)
        let download = Command::new("curl")
            .args(["-L", "-o", archive_path.to_str().unwrap(), url])
            .status();

        match download {
            Ok(status) if status.success() => {
                eprintln!("✅ Downloaded");
            }
            _ => {
                return Self {
                    language: LanguageSupport::Python,
                    available: false,
                    version: None,
                    warnings: vec!["Download failed".to_string()],
                    setup_instructions: Some(format!(
                        "Manual download:\n  curl -L -o {} {}",
                        archive_path.display(),
                        url
                    )),
                };
            }
        }

        eprintln!("📦 Extracting...");

        // Extract
        let extract = Command::new("tar")
            .args([
                "-xzf",
                archive_path.to_str().unwrap(),
                "-C",
                python_dir.to_str().unwrap(),
            ])
            .status();

        match extract {
            Ok(status) if status.success() => {
                eprintln!("✅ Extracted");
            }
            _ => {
                return Self {
                    language: LanguageSupport::Python,
                    available: false,
                    version: None,
                    warnings: vec!["Extraction failed".to_string()],
                    setup_instructions: Some(format!(
                        "Manual extract:\n  tar -xzf {} -C {}",
                        archive_path.display(),
                        python_dir.display()
                    )),
                };
            }
        }

        // Clean up archive
        fs::remove_file(&archive_path).ok();

        // Step 1: Verify Python binary works
        eprintln!("🧪 Testing Python installation...");

        let version_check = Command::new(&python_bin).arg("--version").output();

        let version = match version_check {
            Ok(output) if output.status.success() => {
                let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
                eprintln!("✅ {v} binary works");
                v
            }
            _ => {
                return Self {
                    language: LanguageSupport::Python,
                    available: false,
                    version: None,
                    warnings: vec!["Python binary not working after extraction".to_string()],
                    setup_instructions: Some(format!(
                        "Check installation at: {}",
                        python_dir.display()
                    )),
                };
            }
        };

        // Step 2: Test sandboxed verification (same sandbox as python.rs uses)
        eprintln!("🔒 Testing sandboxed verification...");

        let sandbox_test_result = Self::test_python_sandbox(&python_bin);
        if let Err(msg) = sandbox_test_result {
            return Self {
                language: LanguageSupport::Python,
                available: false,
                version: Some(format!("{version} (standalone)")),
                warnings: vec![msg],
                setup_instructions: Some("Sandbox verification failed".to_string()),
            };
        }
        eprintln!("✅ Sandbox verification works");

        // Step 3: Test that dangerous imports are blocked
        eprintln!("🛡️  Testing import restrictions...");

        let block_test_result = Self::test_import_blocking(&python_bin);
        if let Err(msg) = block_test_result {
            return Self {
                language: LanguageSupport::Python,
                available: false,
                version: Some(format!("{version} (standalone)")),
                warnings: vec![msg],
                setup_instructions: Some("SECURITY: Import blocking failed".to_string()),
            };
        }
        eprintln!("✅ Dangerous imports blocked (os, subprocess, etc.)");

        // Step 4: Test hashlib works (needed for verification)
        eprintln!("🔐 Testing hashlib access...");

        let hashlib_test_result = Self::test_hashlib(&python_bin);
        if let Err(msg) = hashlib_test_result {
            return Self {
                language: LanguageSupport::Python,
                available: false,
                version: Some(format!("{version} (standalone)")),
                warnings: vec![msg],
                setup_instructions: Some("Crypto library unavailable".to_string()),
            };
        }
        eprintln!("✅ hashlib available for cryptographic verification");

        eprintln!("✅ Python sandbox ready for verification");

        // Save config
        let config = PythonSandboxConfig {
            python_dir,
            python_bin,
        };

        if let Ok(json) = serde_json::to_string_pretty(&config) {
            fs::write(&config_path, json).ok();
            eprintln!("💾 Saved config to {}", config_path.display());
        }

        Self {
            language: LanguageSupport::Python,
            available: true,
            version: Some(format!("{version} (standalone, sandboxed)")),
            warnings: Vec::new(),
            setup_instructions: None,
        }
    }

    /// Test that sandbox verification works
    fn test_python_sandbox(python_bin: &std::path::Path) -> Result<(), String> {
        // Inline Python code to test sandbox - uses same pattern as python.rs
        let test_code = r"
import sys, builtins
SAFE = {'abs','all','any','bool','bytes','chr','dict','enumerate','filter',
        'float','format','frozenset','hash','hex','int','isinstance','iter',
        'len','list','map','max','min','oct','ord','pow','print','range',
        'repr','reversed','round','set','slice','sorted','str','sum','tuple',
        'type','zip','ImportError','True','False','None'}
_ri = builtins.__import__
def _si(n,g=None,l=None,f=(),lv=0):
    if n in {'hashlib'}: return _ri(n,g,l,f,lv)
    raise ImportError(f'{n} blocked')
r = {k:getattr(builtins,k) for k in SAFE if hasattr(builtins,k)}
r['__import__'] = _si
ns = {'__builtins__':r,'input_data':b'test','output_data':b'test'}
code = 'def verify(): return input_data == output_data'
exec(compile(code,'<v>','exec'),ns)
print('OK' if ns['verify']() else 'FAIL')
";
        let result = Command::new(python_bin).arg("-c").arg(test_code).output();

        match result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim() == "OK" {
                    Ok(())
                } else {
                    Err(format!("Sandbox returned: {}", stdout.trim()))
                }
            }
            Ok(output) => Err(format!(
                "Sandbox error: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(e) => Err(format!("Failed to run sandbox: {e}")),
        }
    }

    /// Test that dangerous imports are blocked
    fn test_import_blocking(python_bin: &std::path::Path) -> Result<(), String> {
        let test_code = r"
import builtins
_ri = builtins.__import__
def _si(n,g=None,l=None,f=(),lv=0):
    if n in {'hashlib'}: return _ri(n,g,l,f,lv)
    raise ImportError(f'{n} blocked')
builtins.__import__ = _si
try:
    import os
    print('FAIL')
except ImportError:
    print('OK')
";
        let result = Command::new(python_bin).arg("-c").arg(test_code).output();

        match result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim() == "OK" {
                    Ok(())
                } else {
                    Err("os import was NOT blocked - security risk!".to_string())
                }
            }
            _ => Err("Import blocking test failed".to_string()),
        }
    }

    /// Test that hashlib is available
    fn test_hashlib(python_bin: &std::path::Path) -> Result<(), String> {
        let test_code = r"
import hashlib
h = hashlib.sha256(b'test').hexdigest()
print('OK' if len(h) == 64 else 'FAIL')
";
        let result = Command::new(python_bin).arg("-c").arg(test_code).output();

        match result {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim() == "OK" {
                    Ok(())
                } else {
                    Err("hashlib not producing correct output".to_string())
                }
            }
            _ => Err("hashlib not available".to_string()),
        }
    }

    /// Legacy method - detects without setting up (backward compatibility)
    pub fn check_python() -> Self {
        Self::detect_python()
    }

    /// Detect JavaScript environment (read-only, `deno_core` embedded)
    pub fn detect_javascript() -> Self {
        use super::{JavaScriptRuntime, VerificationRuntime};

        let mut warnings = Vec::new();

        // Test actual verification code execution
        let runtime = JavaScriptRuntime::new();
        let test_code = r"
function verify() {
    // Test actual array comparison verification
    if (inputData.length !== outputData.length) return false;
    
    for (let i = 0; i < inputData.length; i++) {
        if (inputData[i] !== outputData[i]) return false;
    }
    
    return true;
}
";

        let test_input = b"test_verification_input";
        let test_output = b"test_verification_input"; // Same = should return true

        let available = match runtime.execute(test_code, test_input, test_output) {
            Ok(true) => true,
            Ok(false) => {
                warnings.push("Verification returned false (unexpected)".to_string());
                false
            }
            Err(e) => {
                warnings.push(format!("Sandbox execution failed: {e}"));
                false
            }
        };

        let setup_instructions = if available {
            None
        } else {
            Some(
                "Deno runtime failed - rebuild binary:\n  cargo clean\n  cargo build --release"
                    .to_string(),
            )
        };

        Self {
            language: LanguageSupport::JavaScript,
            available,
            version: Some("embedded (deno_core verified)".to_string()),
            warnings,
            setup_instructions,
        }
    }

    /// Legacy method - detects without setting up (backward compatibility)
    pub fn check_nodejs() -> Self {
        Self::detect_javascript()
    }

    /// Detect TypeScript environment (read-only, `deno_core` embedded)
    pub fn detect_typescript() -> Self {
        // TypeScript is handled by Deno, same as JavaScript
        let mut check = Self::detect_javascript();
        check.language = LanguageSupport::TypeScript;
        check
    }

    /// Legacy method - detects without setting up (backward compatibility)
    pub fn check_typescript() -> Self {
        Self::detect_typescript()
    }

    /// Detect WASM environment (read-only, wasmer embedded)
    pub fn detect_wasm() -> Self {
        // WASM runtime is embedded via wasmer
        Self {
            language: LanguageSupport::Wasm,
            available: true,
            version: Some("embedded (wasmer)".to_string()),
            warnings: Vec::new(),
            setup_instructions: None,
        }
    }

    /// Legacy method - detects without setting up (backward compatibility)
    pub fn check_wasm() -> Self {
        Self::detect_wasm()
    }

    /// Detect all language environments (read-only, no installs)
    pub fn detect_all() -> Vec<Self> {
        vec![
            Self::detect_python(),
            Self::detect_javascript(),
            Self::detect_typescript(),
            Self::detect_wasm(),
        ]
    }

    /// Legacy method - detects without setting up (backward compatibility)
    pub fn check_all() -> Vec<Self> {
        Self::detect_all()
    }
}

/// Validator's language capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorCapabilities {
    /// Languages this validator can execute
    pub supported_languages: Vec<LanguageSupport>,
    /// Preference weights (0.0 to 1.0) for each language
    /// Higher weight = more capacity/preference for this language
    pub weights: HashMap<LanguageSupport, f64>,
}

impl ValidatorCapabilities {
    /// Create capabilities from environment check
    pub fn from_environment() -> Self {
        let checks = EnvironmentCheck::check_all();
        let mut supported = Vec::new();
        let mut weights = HashMap::new();

        for check in checks {
            if check.available {
                supported.push(check.language);
                // Default weight of 1.0 for available languages
                weights.insert(check.language, 1.0);
            }
        }

        Self {
            supported_languages: supported,
            weights,
        }
    }

    /// Create with specific languages
    pub fn new(languages: Vec<LanguageSupport>) -> Self {
        let mut weights = HashMap::new();
        for lang in &languages {
            weights.insert(*lang, 1.0);
        }

        Self {
            supported_languages: languages,
            weights,
        }
    }

    /// Set preference weight for a language (0.0 to 1.0)
    pub fn set_weight(&mut self, language: LanguageSupport, weight: f64) {
        if self.supported_languages.contains(&language) {
            self.weights.insert(language, weight.clamp(0.0, 1.0));
        }
    }

    /// Get weight for a language
    pub fn get_weight(&self, language: &LanguageSupport) -> f64 {
        self.weights.get(language).copied().unwrap_or(0.0)
    }

    /// Check if language is supported
    pub fn supports(&self, language: &LanguageSupport) -> bool {
        self.supported_languages.contains(language)
    }

    /// Get weighted preference score for a language
    /// Returns 0.0 if not supported
    pub fn preference_score(&self, language: &LanguageSupport) -> f64 {
        if self.supports(language) {
            self.get_weight(language)
        } else {
            0.0
        }
    }
}

/// Calculate supply/demand weights for job distribution
pub struct JobDistribution;

impl JobDistribution {
    /// Calculate weights for job assignment based on validator capabilities
    ///
    /// This implements a supply/demand weighting system:
    /// - If few validators support a language, weight increases (scarcity premium)
    /// - If many validators support a language, weight decreases (abundance discount)
    pub fn calculate_weights(
        language: LanguageSupport,
        all_validators: &[ValidatorCapabilities],
    ) -> Vec<(usize, f64)> {
        let total_validators = all_validators.len() as f64;
        if total_validators == 0.0 {
            return Vec::new();
        }

        // Count validators supporting this language
        let supporting_count = all_validators
            .iter()
            .filter(|v| v.supports(&language))
            .count() as f64;

        if supporting_count == 0.0 {
            return Vec::new();
        }

        // Calculate scarcity multiplier (fewer validators = higher multiplier)
        let scarcity = total_validators / supporting_count;

        // Calculate weights for each validator
        all_validators
            .iter()
            .enumerate()
            .filter_map(|(idx, validator)| {
                if validator.supports(&language) {
                    let base_weight = validator.get_weight(&language);
                    let final_weight = base_weight * scarcity;
                    Some((idx, final_weight))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Select a validator for a job based on weighted random selection
    pub fn select_validator(
        language: LanguageSupport,
        all_validators: &[ValidatorCapabilities],
        random_value: f64, // 0.0 to 1.0
    ) -> Option<usize> {
        let weights = Self::calculate_weights(language, all_validators);
        if weights.is_empty() {
            return None;
        }

        let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();
        let mut cumulative = 0.0;
        let target = random_value * total_weight;

        for (idx, weight) in &weights {
            cumulative += weight;
            if cumulative >= target {
                return Some(*idx);
            }
        }

        // Fallback to last validator
        weights.last().map(|(idx, _)| *idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_checks() {
        let checks = EnvironmentCheck::check_all();
        assert!(!checks.is_empty());

        // WASM should always be available (embedded)
        let wasm_check = checks.iter().find(|c| c.language == LanguageSupport::Wasm);
        assert!(wasm_check.is_some());
        assert!(wasm_check.unwrap().available);
    }

    #[test]
    fn test_capabilities_from_environment() {
        let caps = ValidatorCapabilities::from_environment();
        // At minimum, WASM should be supported
        assert!(caps.supports(&LanguageSupport::Wasm));
    }

    #[test]
    fn test_job_distribution() {
        let validators = vec![
            ValidatorCapabilities::new(vec![LanguageSupport::Python, LanguageSupport::JavaScript]),
            ValidatorCapabilities::new(vec![LanguageSupport::Python]),
            ValidatorCapabilities::new(vec![LanguageSupport::JavaScript]),
        ];

        // Python is supported by 2/3 validators
        let weights = JobDistribution::calculate_weights(LanguageSupport::Python, &validators);
        assert_eq!(weights.len(), 2);

        // JavaScript is supported by 2/3 validators
        let weights = JobDistribution::calculate_weights(LanguageSupport::JavaScript, &validators);
        assert_eq!(weights.len(), 2);
    }

    #[test]
    fn test_scarcity_premium() {
        let validators = vec![
            ValidatorCapabilities::new(vec![LanguageSupport::Python]),
            ValidatorCapabilities::new(vec![LanguageSupport::Python]),
            ValidatorCapabilities::new(vec![LanguageSupport::Python]),
            ValidatorCapabilities::new(vec![LanguageSupport::JavaScript]), // Rare
        ];

        let python_weights =
            JobDistribution::calculate_weights(LanguageSupport::Python, &validators);
        let js_weights =
            JobDistribution::calculate_weights(LanguageSupport::JavaScript, &validators);

        // JavaScript should have higher weight due to scarcity (4/1 = 4x multiplier)
        assert!(js_weights[0].1 > python_weights[0].1);
    }
}
