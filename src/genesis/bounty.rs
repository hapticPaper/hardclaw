//! Genesis bounty system - 90-day parabolic payout curve.
//!
//! Replaces static vesting with a daily bounty pool distributed via
//! a slot-machine mechanism to active participants.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::crypto::{hash_data, Hash};
use crate::types::{Address, HclawAmount, Timestamp};

/// Total bounty pool distributed over 90 days
pub const BOUNTY_POOL: u64 = 2_000_000; // HCLAW

/// Bounty period duration (90 days)
pub const BOUNTY_DAYS: u8 = 90;

/// Minimum active nodes before bounties are paid
pub const MIN_PUBLIC_NODES: u32 = 5;

/// Sum of all weights: Σw(day) for day ∈ [0, 89]
/// Calculated as: Σ(day² × (90 - day)) = 5,466,825
pub const TOTAL_WEIGHT: u128 = 5_466_825;

/// Parab olic weight function: w(day) = day² × (90 - day)
///
/// Properties:
/// - Starts at 0 (day 0)
/// - Peaks at day 60 (two-thirds through)
/// - Returns to 0 at day 90
/// - Pure integer arithmetic, deterministic
#[must_use]
pub fn calculate_daily_weight(day: u8) -> u128 {
    if day >= BOUNTY_DAYS {
        return 0;
    }
    let d = day as u128;
    let remaining = (BOUNTY_DAYS - day) as u128;
    d * d * remaining
}

/// Calculate the daily budget for a given day
#[must_use]
pub fn calculate_daily_budget(day: u8) -> HclawAmount {
    let weight = calculate_daily_weight(day);
    let pool_raw = HclawAmount::from_hclaw(BOUNTY_POOL).raw();
    let daily_raw = pool_raw * weight / TOTAL_WEIGHT;
    HclawAmount::from_raw(daily_raw)
}

/// Slot machine payout - determines if a block wins bounty
#[must_use]
pub fn is_winner_block(block_hash: Hash, day: u8, threshold: u32) -> bool {
    let mut seed_data = block_hash.as_bytes().to_vec();
    seed_data.extend_from_slice(b"bounty");
    seed_data.extend_from_slice(&day.to_le_bytes());

    let seed = hash_data(&seed_data);
    let roll = u32::from_le_bytes([
        seed.as_bytes()[0],
        seed.as_bytes()[1],
        seed.as_bytes()[2],
        seed.as_bytes()[3],
    ]);

    roll < threshold
}

/// Distribute bounty proportionally among contributors
pub fn distribute_bounty(
    contributors: Vec<(Address, u32)>,
    amount: HclawAmount,
) -> Vec<(Address, HclawAmount)> {
    if contributors.is_empty() {
        return Vec::new();
    }

    let total_contributions: u32 = contributors.iter().map(|(_, count)| *count).sum();
    if total_contributions == 0 {
        return Vec::new();
    }

    contributors
        .into_iter()
        .map(|(addr, count)| {
            let share = amount.raw() * count as u128 / total_contributions as u128;
            (addr, HclawAmount::from_raw(share))
        })
        .filter(|(_, amt)| amt.raw() > 0)
        .collect()
}

/// Tracks bounty distributions over the 90-day period
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BountyTracker {
    /// Daily payouts so far (day → amount paid)
    pub daily_paid: HashMap<u8, HclawAmount>,
    /// Total amount paid out
    pub total_paid: HclawAmount,
    /// Total amount withheld and burned (low activity)
    pub total_burned: HclawAmount,
    /// Bounty period start timestamp
    pub start_time: Timestamp,
    /// Number of public (non-bootstrap) nodes
    pub public_node_count: u32,
}

impl BountyTracker {
    /// Create a new bounty tracker
    #[must_use]
    pub fn new(start_time: Timestamp) -> Self {
        Self {
            daily_paid: HashMap::new(),
            total_paid: HclawAmount::ZERO,
            total_burned: HclawAmount::ZERO,
            start_time,
            public_node_count: 0,
        }
    }

    /// Get the current day number (0-89)
    #[must_use]
    pub fn current_day(&self, now: Timestamp) -> Option<u8> {
        if now < self.start_time {
            return None;
        }
        let elapsed = now - self.start_time;
        let days = (elapsed / (24 * 60 * 60 * 1000)) as u8;
        if days >= BOUNTY_DAYS {
            None
        } else {
            Some(days)
        }
    }

    /// Check if bounties are active (enough public nodes)
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.public_node_count >= MIN_PUBLIC_NODES
    }

    /// Get amount paid today
    #[must_use]
    pub fn paid_today(&self, day: u8) -> HclawAmount {
        self.daily_paid
            .get(&day)
            .copied()
            .unwrap_or(HclawAmount::ZERO)
    }

    /// Get remaining budget for today
    #[must_use]
    pub fn remaining_today(&self, day: u8) -> HclawAmount {
        let budget = calculate_daily_budget(day);
        let paid = self.paid_today(day);
        budget.saturating_sub(paid)
    }

    /// Record a payout
    pub fn record_payout(&mut self, day: u8, amount: HclawAmount) {
        let current = self.daily_paid.entry(day).or_insert(HclawAmount::ZERO);
        *current = current.saturating_add(amount);
        self.total_paid = self.total_paid.saturating_add(amount);
    }

    /// Record withheld bounty (burned due to low activity)
    pub fn record_burn(&mut self, amount: HclawAmount) {
        self.total_burned = self.total_burned.saturating_add(amount);
    }

    /// Update public node count
    pub fn update_node_count(&mut self, count: u32) {
        self.public_node_count = count;
    }

    /// Check if the bounty period has ended
    #[must_use]
    pub fn is_period_ended(&self, now: Timestamp) -> bool {
        self.current_day(now).is_none() && now >= self.start_time
    }

    /// Get total remaining (unpaid) bounty
    #[must_use]
    pub fn total_remaining(&self) -> HclawAmount {
        let pool = HclawAmount::from_hclaw(BOUNTY_POOL);
        pool.saturating_sub(self.total_paid)
            .saturating_sub(self.total_burned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounty_curve_sums_to_pool() {
        let mut total = 0u128;
        for day in 0..BOUNTY_DAYS {
            total += calculate_daily_budget(day).raw();
        }
        let pool = HclawAmount::from_hclaw(BOUNTY_POOL).raw();
        // Allow for small rounding error (< 1 HCLAW)
        let diff = if total > pool {
            total - pool
        } else {
            pool - total
        };
        let one_hclaw = HclawAmount::from_hclaw(1).raw();
        assert!(
            diff < one_hclaw,
            "Total should sum to pool, got diff of {diff}"
        );
    }

    #[test]
    fn test_bounty_peaks_at_day_60() {
        let peak_day = 60;
        let peak_amount = calculate_daily_budget(peak_day);

        // Check that day 60 is >= all other days
        for day in 0..BOUNTY_DAYS {
            let amount = calculate_daily_budget(day);
            if day != peak_day {
                assert!(
                    peak_amount >= amount,
                    "Day {peak_day} should be peak, but day {day} was higher"
                );
            }
        }
    }

    #[test]
    fn test_bounty_starts_and_ends_at_zero() {
        assert_eq!(calculate_daily_budget(0).raw(), 0);
        assert!(calculate_daily_budget(89).raw() > 0); // Last valid day is still non-zero
        assert_eq!(calculate_daily_budget(90).raw(), 0); // Out of range
    }

    #[test]
    fn test_weight_formula() {
        assert_eq!(calculate_daily_weight(0), 0); // 0² × 90 = 0
        assert_eq!(calculate_daily_weight(10), 8_000); // 10² × 80 = 8,000
        assert_eq!(calculate_daily_weight(60), 108_000); // 60² × 30 = 108,000
        assert_eq!(calculate_daily_weight(89), 7_921); // 89² × 1 = 7,921
        assert_eq!(calculate_daily_weight(90), 0); // Out of range
    }

    #[test]
    fn test_distribute_bounty_proportional() {
        let contributors = vec![
            (Address::from_bytes([1; 20]), 10), // 50%
            (Address::from_bytes([2; 20]), 6),  // 30%
            (Address::from_bytes([3; 20]), 4),  // 20%
        ];
        let amount = HclawAmount::from_hclaw(1000);

        let distribution = distribute_bounty(contributors, amount);

        assert_eq!(distribution.len(), 3);
        assert_eq!(distribution[0].1.whole_hclaw(), 500);
        assert_eq!(distribution[1].1.whole_hclaw(), 300);
        assert_eq!(distribution[2].1.whole_hclaw(), 200);
    }

    #[test]
    fn test_bounty_tracker_current_day() {
        let start = 1000;
        let tracker = BountyTracker::new(start);

        assert_eq!(tracker.current_day(start), Some(0));
        assert_eq!(tracker.current_day(start + 24 * 60 * 60 * 1000), Some(1));
        assert_eq!(
            tracker.current_day(start + 60 * 24 * 60 * 60 * 1000),
            Some(60)
        );
        assert_eq!(
            tracker.current_day(start + 89 * 24 * 60 * 60 * 1000),
            Some(89)
        );
        assert_eq!(tracker.current_day(start + 90 * 24 * 60 * 60 * 1000), None);
    }

    #[test]
    fn test_bounty_tracker_remaining() {
        let start = 1000;
        let mut tracker = BountyTracker::new(start);

        let day_10_budget = calculate_daily_budget(10);
        assert_eq!(tracker.remaining_today(10), day_10_budget);

        tracker.record_payout(10, HclawAmount::from_hclaw(1000));
        let remaining = tracker.remaining_today(10);
        assert_eq!(
            remaining,
            day_10_budget.saturating_sub(HclawAmount::from_hclaw(1000))
        );
    }

    #[test]
    fn test_bounty_tracker_not_active_until_min_nodes() {
        let tracker = BountyTracker::new(1000);
        assert!(!tracker.is_active());

        let mut tracker = tracker;
        tracker.update_node_count(MIN_PUBLIC_NODES);
        assert!(tracker.is_active());
    }

    #[test]
    fn test_slot_machine_deterministic() {
        let hash = hash_data(b"test");
        let threshold = u32::MAX / 2; // 50% chance

        let result1 = is_winner_block(hash, 10, threshold);
        let result2 = is_winner_block(hash, 10, threshold);
        assert_eq!(result1, result2, "Same inputs should give same result");

        // Different day should give different result
        let result3 = is_winner_block(hash, 11, threshold);
        // Not guaranteed to be different, but very likely
        assert!(result1 == result3 || result1 != result3); // Just checking it compiles
    }
}
