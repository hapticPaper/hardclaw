//! Token burning mechanism.
//!
//! Burns serve two purposes:
//! 1. Offset state bloat (1% of fees burned)
//! 2. Anti-Sybil defense (burn-to-request for job submission)

use std::collections::HashMap;

use crate::types::{HclawAmount, Timestamp, now_millis};

/// Reason for a token burn
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BurnReason {
    /// Fee from completed job
    JobFee,
    /// Burn-to-request for job submission
    JobSubmission,
    /// Slashing (stake burned due to misbehavior)
    Slashing,
    /// Manual burn
    Manual,
}

/// A burn event
#[derive(Clone, Debug)]
pub struct BurnEvent {
    /// Amount burned
    pub amount: HclawAmount,
    /// Reason for burn
    pub reason: BurnReason,
    /// When the burn occurred
    pub timestamp: Timestamp,
}

/// Manages token burns
pub struct BurnManager {
    /// Total burned ever
    total_burned: HclawAmount,
    /// Burns by reason
    burns_by_reason: HashMap<BurnReason, HclawAmount>,
    /// Recent burn history
    burn_history: Vec<BurnEvent>,
    /// Max history length
    max_history: usize,
}

impl Default for BurnManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BurnManager {
    /// Create new burn manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            total_burned: HclawAmount::ZERO,
            burns_by_reason: HashMap::new(),
            burn_history: Vec::new(),
            max_history: 10_000,
        }
    }

    /// Record a burn
    pub fn burn(&mut self, amount: HclawAmount, reason: BurnReason) {
        self.total_burned = self.total_burned.saturating_add(amount);

        *self.burns_by_reason.entry(reason.clone()).or_insert(HclawAmount::ZERO) =
            self.burns_by_reason
                .get(&reason)
                .unwrap_or(&HclawAmount::ZERO)
                .saturating_add(amount);

        let event = BurnEvent {
            amount,
            reason,
            timestamp: now_millis(),
        };

        self.burn_history.push(event);

        // Trim history if needed
        if self.burn_history.len() > self.max_history {
            self.burn_history.remove(0);
        }
    }

    /// Get total burned
    #[must_use]
    pub const fn total_burned(&self) -> HclawAmount {
        self.total_burned
    }

    /// Get burns by reason
    #[must_use]
    pub fn burned_for(&self, reason: &BurnReason) -> HclawAmount {
        self.burns_by_reason.get(reason).copied().unwrap_or(HclawAmount::ZERO)
    }

    /// Get burn statistics
    #[must_use]
    pub fn stats(&self) -> BurnStats {
        BurnStats {
            total_burned: self.total_burned,
            job_fee_burns: self.burned_for(&BurnReason::JobFee),
            submission_burns: self.burned_for(&BurnReason::JobSubmission),
            slash_burns: self.burned_for(&BurnReason::Slashing),
            burn_count: self.burn_history.len(),
        }
    }

    /// Get recent burn history
    #[must_use]
    pub fn recent_burns(&self, limit: usize) -> &[BurnEvent] {
        let start = self.burn_history.len().saturating_sub(limit);
        &self.burn_history[start..]
    }
}

/// Burn statistics
#[derive(Clone, Debug)]
pub struct BurnStats {
    /// Total ever burned
    pub total_burned: HclawAmount,
    /// Burned from job fees
    pub job_fee_burns: HclawAmount,
    /// Burned from job submissions
    pub submission_burns: HclawAmount,
    /// Burned from slashing
    pub slash_burns: HclawAmount,
    /// Number of burn events
    pub burn_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_burn_tracking() {
        let mut manager = BurnManager::new();

        manager.burn(HclawAmount::from_hclaw(10), BurnReason::JobFee);
        manager.burn(HclawAmount::from_hclaw(5), BurnReason::JobSubmission);
        manager.burn(HclawAmount::from_hclaw(100), BurnReason::Slashing);

        assert_eq!(manager.total_burned().whole_hclaw(), 115);
        assert_eq!(manager.burned_for(&BurnReason::JobFee).whole_hclaw(), 10);
        assert_eq!(manager.burned_for(&BurnReason::Slashing).whole_hclaw(), 100);
    }

    #[test]
    fn test_burn_history() {
        let mut manager = BurnManager::new();

        for i in 0..5 {
            manager.burn(HclawAmount::from_hclaw(i + 1), BurnReason::JobFee);
        }

        let recent = manager.recent_burns(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].amount.whole_hclaw(), 3); // Third burn
        assert_eq!(recent[2].amount.whole_hclaw(), 5); // Fifth burn
    }
}
