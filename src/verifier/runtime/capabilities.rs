//! Validator capabilities and environment checking.
//!
//! This module manages what languages a validator can execute and provides
//! environment setup validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

/// Languages supported by the verification system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LanguageSupport {
    Python,
    JavaScript,
    TypeScript,
    Wasm,
}

impl LanguageSupport {
    /// Get all supported languages
    pub fn all() -> Vec<Self> {
        vec![
            Self::Python,
            Self::JavaScript,
            Self::TypeScript,
            Self::Wasm,
        ]
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

impl EnvironmentCheck {
    /// Check Python environment
    pub fn check_python() -> Self {
        let mut warnings = Vec::new();
        
        // Try to run python3 --version
        let output = Command::new("python3")
            .arg("--version")
            .output();

        let (available, version) = match output {
            Ok(output) if output.status.success() => {
                let version_str = String::from_utf8_lossy(&output.stdout);
                let version = version_str.trim().to_string();
                
                // Check if version is >= 3.8
                if !version.contains("3.8") && !version.contains("3.9") 
                    && !version.contains("3.10") && !version.contains("3.11")
                    && !version.contains("3.12") && !version.contains("3.13") {
                    warnings.push("Python 3.8 or higher recommended".to_string());
                }
                
                (true, Some(version))
            }
            _ => (false, None),
        };

        let setup_instructions = if !available {
            Some(
                "Install Python 3.8 or higher:\n\
                 macOS: brew install python3\n\
                 Ubuntu: sudo apt install python3\n\
                 Windows: Download from python.org".to_string()
            )
        } else {
            None
        };

        Self {
            language: LanguageSupport::Python,
            available,
            version,
            warnings,
            setup_instructions,
        }
    }

    /// Check Node.js environment for JavaScript/TypeScript
    pub fn check_nodejs() -> Self {
        let mut warnings = Vec::new();
        
        // Try to run node --version
        let output = Command::new("node")
            .arg("--version")
            .output();

        let (available, version) = match output {
            Ok(output) if output.status.success() => {
                let version_str = String::from_utf8_lossy(&output.stdout);
                let version = version_str.trim().to_string();
                
                // Check if version is >= 18
                if let Some(v) = version.strip_prefix('v') {
                    if let Some(major) = v.split('.').next() {
                        if let Ok(major_num) = major.parse::<u32>() {
                            if major_num < 18 {
                                warnings.push("Node.js 18 or higher recommended".to_string());
                            }
                        }
                    }
                }
                
                (true, Some(version))
            }
            _ => {
                // Node not required since we use embedded Deno
                (true, Some("embedded (Deno)".to_string()))
            }
        };

        Self {
            language: LanguageSupport::JavaScript,
            available,
            version,
            warnings,
            setup_instructions: None, // Deno is embedded
        }
    }

    /// Check TypeScript environment
    pub fn check_typescript() -> Self {
        // TypeScript is handled by Deno, same as JavaScript
        let mut check = Self::check_nodejs();
        check.language = LanguageSupport::TypeScript;
        check
    }

    /// Check WASM environment
    pub fn check_wasm() -> Self {
        // WASM runtime is embedded via wasmer
        Self {
            language: LanguageSupport::Wasm,
            available: true,
            version: Some("embedded (wasmer)".to_string()),
            warnings: Vec::new(),
            setup_instructions: None,
        }
    }

    /// Check all language environments
    pub fn check_all() -> Vec<Self> {
        vec![
            Self::check_python(),
            Self::check_nodejs(),
            Self::check_typescript(),
            Self::check_wasm(),
        ]
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

        let python_weights = JobDistribution::calculate_weights(LanguageSupport::Python, &validators);
        let js_weights = JobDistribution::calculate_weights(LanguageSupport::JavaScript, &validators);

        // JavaScript should have higher weight due to scarcity (4/1 = 4x multiplier)
        assert!(js_weights[0].1 > python_weights[0].1);
    }
}
