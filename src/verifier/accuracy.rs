//! Rolling accuracy tracking for verifier consensus agreement.
//!
//! Replaces per-vote slashing for being out of consensus with a
//! rolling window approach. This protects honest contrarians while
//! still catching lazy or malicious verifiers through pattern detection.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::crypto::Hash;
use crate::types::{Address, Timestamp};

/// Configuration for rolling accuracy tracking
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccuracyConfig {
    /// Window size (number of verifications to track per verifier)
    pub window_size: usize,
    /// Above this = Good (no action)
    pub warning_threshold: f64,
    /// Between warning and slash = Warning (logged but no slash)
    pub slash_threshold: f64,
    /// Below slash = active slashing
    pub critical_threshold: f64,
    /// Slash percentage when below `slash_threshold`
    pub slash_percent: u8,
    /// Slash percentage when below `critical_threshold`
    pub critical_slash_percent: u8,
    /// Minimum verifications before accuracy is evaluated
    pub min_verifications: usize,
}

impl Default for AccuracyConfig {
    fn default() -> Self {
        Self {
            window_size: 100,
            warning_threshold: 0.70,
            slash_threshold: 0.60,
            critical_threshold: 0.40,
            slash_percent: 2,
            critical_slash_percent: 10,
            min_verifications: 20,
        }
    }
}

/// Result of a single verification from this verifier's perspective
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationOutcome {
    /// Did this verifier agree with consensus?
    pub agreed_with_consensus: bool,
    /// The solution this was for
    pub solution_id: Hash,
    /// When this occurred
    pub timestamp: Timestamp,
}

/// Accuracy status for a verifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccuracyStatus {
    /// Not enough data to evaluate
    Probationary,
    /// Accuracy above warning threshold
    Good,
    /// Accuracy between warning and slash thresholds
    Warning,
    /// Accuracy below slash threshold — active slashing
    Slashing,
    /// Accuracy below critical threshold — slash and deactivate
    Critical,
}

/// Tracks rolling accuracy for a single verifier
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifierAccuracy {
    /// Rolling window of recent outcomes
    outcomes: VecDeque<VerificationOutcome>,
    /// Agreements in the current window
    agreement_count: usize,
    /// Current accuracy (0.0 - 1.0)
    pub current_accuracy: f64,
    /// Current status
    pub status: AccuracyStatus,
    /// Lifetime totals
    pub total_verifications: u64,
    /// Lifetime agreements
    pub total_agreements: u64,
}

impl VerifierAccuracy {
    /// Create a new tracker for a verifier
    #[must_use]
    pub fn new() -> Self {
        Self {
            outcomes: VecDeque::new(),
            agreement_count: 0,
            current_accuracy: 1.0,
            status: AccuracyStatus::Probationary,
            total_verifications: 0,
            total_agreements: 0,
        }
    }

    /// Record a new verification outcome
    pub fn record(&mut self, outcome: VerificationOutcome, config: &AccuracyConfig) {
        self.total_verifications += 1;
        if outcome.agreed_with_consensus {
            self.agreement_count += 1;
            self.total_agreements += 1;
        }

        self.outcomes.push_back(outcome);

        // Trim window
        while self.outcomes.len() > config.window_size {
            let removed = self.outcomes.pop_front().unwrap();
            if removed.agreed_with_consensus {
                self.agreement_count -= 1;
            }
        }

        self.update_status(config);
    }

    fn update_status(&mut self, config: &AccuracyConfig) {
        if self.outcomes.len() < config.min_verifications {
            self.status = AccuracyStatus::Probationary;
            return;
        }

        self.current_accuracy = self.agreement_count as f64 / self.outcomes.len() as f64;

        self.status = if self.current_accuracy >= config.warning_threshold {
            AccuracyStatus::Good
        } else if self.current_accuracy >= config.slash_threshold {
            AccuracyStatus::Warning
        } else if self.current_accuracy >= config.critical_threshold {
            AccuracyStatus::Slashing
        } else {
            AccuracyStatus::Critical
        };
    }
}

impl Default for VerifierAccuracy {
    fn default() -> Self {
        Self::new()
    }
}

/// What action to take based on accuracy
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlashAction {
    /// No slashing needed
    None,
    /// Slash a percentage of stake
    Slash {
        /// Percentage to slash
        percent: u8,
    },
    /// Slash and deactivate the verifier
    SlashAndDeactivate {
        /// Percentage to slash
        percent: u8,
    },
}

/// Manages accuracy tracking for all verifiers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccuracyTracker {
    /// Config
    config: AccuracyConfig,
    /// Accuracy records per verifier
    verifiers: HashMap<Address, VerifierAccuracy>,
}

impl AccuracyTracker {
    /// Create a new tracker
    #[must_use]
    pub fn new(config: AccuracyConfig) -> Self {
        Self {
            config,
            verifiers: HashMap::new(),
        }
    }

    /// Record a verification outcome for a verifier
    pub fn record_outcome(&mut self, verifier: &Address, outcome: VerificationOutcome) {
        let accuracy = self.verifiers.entry(*verifier).or_default();
        accuracy.record(outcome, &self.config);
    }

    /// Get the current slash action for a verifier
    #[must_use]
    pub fn get_slash_action(&self, verifier: &Address) -> SlashAction {
        match self.verifiers.get(verifier) {
            Some(accuracy) => match accuracy.status {
                AccuracyStatus::Slashing => SlashAction::Slash {
                    percent: self.config.slash_percent,
                },
                AccuracyStatus::Critical => SlashAction::SlashAndDeactivate {
                    percent: self.config.critical_slash_percent,
                },
                _ => SlashAction::None,
            },
            None => SlashAction::None,
        }
    }

    /// Get accuracy info for a verifier
    #[must_use]
    pub fn get_accuracy(&self, verifier: &Address) -> Option<&VerifierAccuracy> {
        self.verifiers.get(verifier)
    }

    /// Get the accuracy status for a verifier
    #[must_use]
    pub fn get_status(&self, verifier: &Address) -> AccuracyStatus {
        self.verifiers
            .get(verifier)
            .map_or(AccuracyStatus::Probationary, |a| a.status)
    }
}

impl Default for AccuracyTracker {
    fn default() -> Self {
        Self::new(AccuracyConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_addr() -> Address {
        Address::from_public_key(Keypair::generate().public_key())
    }

    fn make_outcome(agreed: bool) -> VerificationOutcome {
        VerificationOutcome {
            agreed_with_consensus: agreed,
            solution_id: Hash::ZERO,
            timestamp: 0,
        }
    }

    #[test]
    fn test_probationary_period() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // Less than 20 verifications — should be probationary
        for _ in 0..19 {
            tracker.record_outcome(&addr, make_outcome(false));
        }

        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Probationary);
        assert_eq!(tracker.get_slash_action(&addr), SlashAction::None);
    }

    #[test]
    fn test_good_accuracy() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // 80% agreement — above 70% warning threshold
        for i in 0..100 {
            tracker.record_outcome(&addr, make_outcome(i % 5 != 0));
        }

        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Good);
    }

    #[test]
    fn test_warning_no_slash() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // 65% agreement — between 60% slash and 70% warning
        for i in 0..100 {
            tracker.record_outcome(&addr, make_outcome(i < 65));
        }

        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Warning);
        assert_eq!(tracker.get_slash_action(&addr), SlashAction::None);
    }

    #[test]
    fn test_slashing_zone() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // 50% agreement — between 40% critical and 60% slash
        for i in 0..100 {
            tracker.record_outcome(&addr, make_outcome(i < 50));
        }

        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Slashing);
        assert_eq!(
            tracker.get_slash_action(&addr),
            SlashAction::Slash { percent: 2 }
        );
    }

    #[test]
    fn test_critical_zone() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // 30% agreement — below 40% critical
        for i in 0..100 {
            tracker.record_outcome(&addr, make_outcome(i < 30));
        }

        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Critical);
        assert_eq!(
            tracker.get_slash_action(&addr),
            SlashAction::SlashAndDeactivate { percent: 10 }
        );
    }

    #[test]
    fn test_rolling_window_recovery() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // Start with bad accuracy (50 agreements out of 100)
        for i in 0..100 {
            tracker.record_outcome(&addr, make_outcome(i < 50));
        }
        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Slashing);

        // Now 100 more all agreeing — pushes out the old bad ones
        for _ in 0..100 {
            tracker.record_outcome(&addr, make_outcome(true));
        }
        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Good);
    }

    #[test]
    fn test_single_disagreement_no_punishment() {
        let mut tracker = AccuracyTracker::default();
        let addr = test_addr();

        // 99 agreements, 1 disagreement
        for i in 0..100 {
            tracker.record_outcome(&addr, make_outcome(i != 50));
        }

        assert_eq!(tracker.get_status(&addr), AccuracyStatus::Good);
        assert_eq!(tracker.get_slash_action(&addr), SlashAction::None);
    }
}
