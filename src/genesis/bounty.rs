//! Genesis bounty system — 90-day parabolic payout curve.
//!
//! Every hour, 1/24th of the day's budget is distributed evenly among
//! eligible staked verifiers who attested to blocks in the prior hour.
//! The daily budget follows a parabolic curve: day² × (90 - day).

use serde::{Deserialize, Serialize};

use crate::types::{Address, HclawAmount};

/// Total bounty pool distributed over 90 days
pub const BOUNTY_POOL: u64 = 2_000_000; // HCLAW

/// Bounty period duration (90 days)
pub const BOUNTY_DAYS: u8 = 90;

/// Minimum active nodes before bounties are paid
pub const MIN_PUBLIC_NODES: u32 = 5;

/// Sum of all weights: Σw(day) for day ∈ [0, 89]
/// Calculated as: Σ(day² × (90 - day)) = 5,466,825
pub const TOTAL_WEIGHT: u128 = 5_466_825;

/// One hour in milliseconds
pub const HOUR_MS: u64 = 3_600_000;

/// Number of hours per day
pub const HOURS_PER_DAY: u64 = 24;

/// Total epochs in the bounty period (90 days × 24 hours)
pub const TOTAL_EPOCHS: u64 = BOUNTY_DAYS as u64 * HOURS_PER_DAY;

/// Parabolic weight function: w(day) = day² × (90 - day)
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

/// Calculate the hourly budget for a given day (1/24th of daily budget).
/// Integer division means up to 23 raw units of dust per day — acceptable.
#[must_use]
pub fn calculate_hourly_budget(day: u8) -> HclawAmount {
    let daily = calculate_daily_budget(day);
    HclawAmount::from_raw(daily.raw() / HOURS_PER_DAY as u128)
}

/// Compute the bounty epoch (hour index since start) from a timestamp.
/// Returns `None` if the timestamp is before start or beyond the 90-day period.
#[must_use]
pub fn compute_epoch(timestamp: u64, start_time: u64) -> Option<u64> {
    if timestamp < start_time {
        return None;
    }
    let elapsed_ms = timestamp - start_time;
    let epoch = elapsed_ms / HOUR_MS;
    if epoch >= TOTAL_EPOCHS {
        None
    } else {
        Some(epoch)
    }
}

/// Get the day number (0-89) for a given epoch.
#[must_use]
pub fn day_from_epoch(epoch: u64) -> u8 {
    (epoch / HOURS_PER_DAY) as u8
}

/// Distribute an amount evenly among recipients.
/// Any remainder (dust) is not distributed — it gets burned at period end.
pub fn distribute_evenly(
    recipients: &[Address],
    total: HclawAmount,
) -> Vec<(Address, HclawAmount)> {
    if recipients.is_empty() {
        return Vec::new();
    }
    let per_recipient = HclawAmount::from_raw(total.raw() / recipients.len() as u128);
    if per_recipient.raw() == 0 {
        return Vec::new();
    }
    recipients
        .iter()
        .map(|addr| (*addr, per_recipient))
        .collect()
}

/// Tracks bounty distributions over the 90-day period.
///
/// Epochs are distributed sequentially (0, 1, 2, ..., 2159).
/// Each epoch represents one hour. The contract enforces sequential
/// distribution — epoch N can only be distributed if N-1 was already done.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BountyTracker {
    /// Last distributed epoch (hour index since start).
    /// `u64::MAX` means no distribution has occurred yet.
    pub last_distributed_epoch: u64,
    /// Total amount paid out across all epochs
    pub total_paid: HclawAmount,
    /// Total amount burned (skipped hours, dust, inactivity)
    pub total_burned: HclawAmount,
    /// Bounty period start timestamp (milliseconds)
    pub start_time: u64,
    /// Number of public (non-bootstrap) nodes
    pub public_node_count: u32,
}

impl BountyTracker {
    /// Create a new bounty tracker
    #[must_use]
    pub fn new(start_time: u64) -> Self {
        Self {
            last_distributed_epoch: u64::MAX,
            total_paid: HclawAmount::ZERO,
            total_burned: HclawAmount::ZERO,
            start_time,
            public_node_count: 0,
        }
    }

    /// Check if the given epoch is the next one to distribute.
    #[must_use]
    pub fn is_next_epoch(&self, epoch: u64) -> bool {
        if self.last_distributed_epoch == u64::MAX {
            epoch == 0
        } else {
            epoch == self.last_distributed_epoch + 1
        }
    }

    /// Check if bounties are active (enough public nodes)
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.public_node_count >= MIN_PUBLIC_NODES
    }

    /// Record a distribution for an epoch
    pub fn record_distribution(&mut self, epoch: u64, amount: HclawAmount) {
        self.last_distributed_epoch = epoch;
        self.total_paid = self.total_paid.saturating_add(amount);
    }

    /// Record burned bounty (skipped hours, dust, inactivity)
    pub fn record_burn(&mut self, amount: HclawAmount) {
        self.total_burned = self.total_burned.saturating_add(amount);
    }

    /// Update public node count
    pub fn update_node_count(&mut self, count: u32) {
        self.public_node_count = count;
    }

    /// Get total remaining (unpaid, unburned) bounty
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
        assert!(calculate_daily_budget(89).raw() > 0);
        assert_eq!(calculate_daily_budget(90).raw(), 0);
    }

    #[test]
    fn test_weight_formula() {
        assert_eq!(calculate_daily_weight(0), 0);
        assert_eq!(calculate_daily_weight(10), 8_000);
        assert_eq!(calculate_daily_weight(60), 108_000);
        assert_eq!(calculate_daily_weight(89), 7_921);
        assert_eq!(calculate_daily_weight(90), 0);
    }

    #[test]
    fn test_hourly_budget_sums_to_daily() {
        for day in 1..BOUNTY_DAYS {
            let daily = calculate_daily_budget(day).raw();
            let hourly = calculate_hourly_budget(day).raw();
            let hourly_total = hourly * HOURS_PER_DAY as u128;
            // Integer division dust: at most 23 raw units lost
            let diff = daily - hourly_total;
            assert!(
                diff < HOURS_PER_DAY as u128,
                "Day {day}: daily={daily}, hourly*24={hourly_total}, diff={diff}"
            );
        }
    }

    #[test]
    fn test_hourly_budget_day_zero_is_zero() {
        assert_eq!(calculate_hourly_budget(0).raw(), 0);
    }

    #[test]
    fn test_compute_epoch_boundaries() {
        let start = 1_000_000u64;

        assert_eq!(compute_epoch(start, start), Some(0));
        assert_eq!(compute_epoch(start + HOUR_MS - 1, start), Some(0));
        assert_eq!(compute_epoch(start + HOUR_MS, start), Some(1));
        assert_eq!(compute_epoch(start + 24 * HOUR_MS, start), Some(24)); // day 1 hour 0
        assert_eq!(
            compute_epoch(start + TOTAL_EPOCHS * HOUR_MS - 1, start),
            Some(TOTAL_EPOCHS - 1)
        );
        assert_eq!(compute_epoch(start + TOTAL_EPOCHS * HOUR_MS, start), None); // beyond 90 days
        assert_eq!(compute_epoch(start - 1, start), None); // before start
    }

    #[test]
    fn test_day_from_epoch() {
        assert_eq!(day_from_epoch(0), 0);
        assert_eq!(day_from_epoch(23), 0);
        assert_eq!(day_from_epoch(24), 1);
        assert_eq!(day_from_epoch(48), 2);
        assert_eq!(day_from_epoch(TOTAL_EPOCHS - 1), 89);
    }

    #[test]
    fn test_distribute_evenly_basic() {
        let addrs = vec![
            Address::from_bytes([1; 20]),
            Address::from_bytes([2; 20]),
            Address::from_bytes([3; 20]),
        ];
        let total = HclawAmount::from_hclaw(900);

        let result = distribute_evenly(&addrs, total);
        assert_eq!(result.len(), 3);
        for (_, amt) in &result {
            assert_eq!(amt.whole_hclaw(), 300);
        }
    }

    #[test]
    fn test_distribute_evenly_dust() {
        let addrs = vec![
            Address::from_bytes([1; 20]),
            Address::from_bytes([2; 20]),
            Address::from_bytes([3; 20]),
        ];
        let total = HclawAmount::from_hclaw(1000);

        let result = distribute_evenly(&addrs, total);
        assert_eq!(result.len(), 3);
        // 1000 / 3 = 333 each, 1 HCLAW dust
        for (_, amt) in &result {
            assert_eq!(amt.whole_hclaw(), 333);
        }
        let distributed: u128 = result.iter().map(|(_, a)| a.raw()).sum();
        let dust = total.raw() - distributed;
        assert!(dust > 0, "Should have some dust");
        assert!(dust < HclawAmount::from_hclaw(1).raw());
    }

    #[test]
    fn test_distribute_evenly_empty() {
        let result = distribute_evenly(&[], HclawAmount::from_hclaw(1000));
        assert!(result.is_empty());
    }

    #[test]
    fn test_distribute_evenly_single() {
        let addrs = vec![Address::from_bytes([1; 20])];
        let total = HclawAmount::from_hclaw(500);

        let result = distribute_evenly(&addrs, total);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.whole_hclaw(), 500);
    }

    #[test]
    fn test_distribute_evenly_zero_budget() {
        let addrs = vec![Address::from_bytes([1; 20])];
        let result = distribute_evenly(&addrs, HclawAmount::ZERO);
        assert!(result.is_empty());
    }

    #[test]
    fn test_bounty_tracker_epoch_sequencing() {
        let mut tracker = BountyTracker::new(0);

        assert!(tracker.is_next_epoch(0));
        assert!(!tracker.is_next_epoch(1));
        assert!(!tracker.is_next_epoch(5));

        tracker.record_distribution(0, HclawAmount::from_hclaw(100));

        assert!(!tracker.is_next_epoch(0));
        assert!(tracker.is_next_epoch(1));
        assert!(!tracker.is_next_epoch(2));

        tracker.record_distribution(1, HclawAmount::from_hclaw(100));
        assert!(tracker.is_next_epoch(2));
    }

    #[test]
    fn test_bounty_tracker_not_active_until_min_nodes() {
        let tracker = BountyTracker::new(0);
        assert!(!tracker.is_active());

        let mut tracker = tracker;
        tracker.update_node_count(MIN_PUBLIC_NODES);
        assert!(tracker.is_active());
    }

    #[test]
    fn test_total_pool_accounted() {
        // Sum all 2160 hourly budgets and verify they account for the pool
        let mut total_hourly = 0u128;
        for epoch in 0..TOTAL_EPOCHS {
            let day = day_from_epoch(epoch);
            total_hourly += calculate_hourly_budget(day).raw();
        }
        let pool = HclawAmount::from_hclaw(BOUNTY_POOL).raw();
        // The difference is integer division dust: up to 23 raw units per day × 90 days
        let diff = pool - total_hourly;
        let max_dust = HOURS_PER_DAY as u128 * BOUNTY_DAYS as u128;
        assert!(
            diff <= max_dust,
            "Pool accounting drift too large: diff={diff}, max_dust={max_dust}"
        );
    }
}
