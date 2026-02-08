//! Liveness-gated daily vesting for airdrop tokens.
//!
//! Vesting is NOT purely time-based. Each verifier must actively operate
//! their node each day (proven by block attestations) to unlock that day's
//! vesting portion. If a verifier is offline on day 15, they don't get
//! day 15's tokens — those tokens are burned at bootstrap end.
//!
//! Structure:
//! - Immediate unlock: enough to meet min_stake (so they can participate)
//! - Remaining tokens: divided into 30 daily portions
//! - Each daily portion unlocks ONLY if the verifier was active that day
//!   (met the minimum attestation threshold via the LivenessTracker)

use serde::{Deserialize, Serialize};

use super::BOOTSTRAP_DAYS;
use crate::types::{HclawAmount, Timestamp};

/// Liveness-gated vesting schedule for a single verifier
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VestingSchedule {
    /// Total grant amount
    pub total_amount: HclawAmount,
    /// Amount immediately available (enough to meet min_stake)
    pub immediate_amount: HclawAmount,
    /// Amount subject to daily vesting (total - immediate)
    pub vesting_amount: HclawAmount,
    /// Per-day vesting amount (vesting_amount / 30)
    pub daily_amount: HclawAmount,
    /// Bootstrap start timestamp (for day alignment)
    pub bootstrap_start: Timestamp,
    /// Bootstrap end timestamp
    pub bootstrap_end: Timestamp,
    /// The day this verifier joined (0-indexed from bootstrap start)
    pub join_day: u32,
    /// Which days the verifier was active (updated by liveness tracker).
    /// Index = day number (0-29), value = whether they were active.
    pub daily_active: Vec<bool>,
    /// Total amount withdrawn so far
    pub withdrawn: HclawAmount,
}

impl VestingSchedule {
    /// Create a new liveness-gated vesting schedule.
    ///
    /// `min_stake` tokens are immediately available for staking.
    /// The remainder is divided into daily portions, one per day of the
    /// bootstrap period, starting from the day they joined.
    #[must_use]
    pub fn new(
        total_amount: HclawAmount,
        min_stake: HclawAmount,
        bootstrap_start: Timestamp,
        bootstrap_end: Timestamp,
        join_day: u32,
    ) -> Self {
        let immediate = min_stake.min(total_amount);
        let vesting = total_amount.saturating_sub(immediate);

        // Days remaining from join_day to end of bootstrap
        let days_remaining = BOOTSTRAP_DAYS.saturating_sub(join_day);
        let daily = if days_remaining > 0 && vesting.raw() > 0 {
            HclawAmount::from_raw(vesting.raw() / days_remaining as u128)
        } else {
            HclawAmount::ZERO
        };

        Self {
            total_amount,
            immediate_amount: immediate,
            vesting_amount: vesting,
            daily_amount: daily,
            bootstrap_start,
            bootstrap_end,
            join_day,
            daily_active: vec![false; BOOTSTRAP_DAYS as usize],
            withdrawn: HclawAmount::ZERO,
        }
    }

    /// Mark a day as active (called by bootstrap state machine when
    /// liveness tracker confirms the verifier met the threshold).
    pub fn mark_day_active(&mut self, day: u32) {
        if let Some(active) = self.daily_active.get_mut(day as usize) {
            *active = true;
        }
    }

    /// Count of active days since joining
    #[must_use]
    pub fn active_days_count(&self) -> u32 {
        self.daily_active
            .iter()
            .enumerate()
            .filter(|(i, active)| **active && *i as u32 >= self.join_day)
            .count() as u32
    }

    /// Calculate total vested amount based on liveness.
    /// Only days where the verifier was active contribute to vesting.
    #[must_use]
    pub fn vested_amount(&self) -> HclawAmount {
        let mut vested = self.immediate_amount;

        // Add daily_amount for each active day from join_day onward
        for day in self.join_day..BOOTSTRAP_DAYS {
            if self.daily_active.get(day as usize).copied().unwrap_or(false) {
                vested = vested.saturating_add(self.daily_amount);
            }
        }

        // Cap at total (handles rounding from integer division)
        vested.min(self.total_amount)
    }

    /// Calculate withdrawable amount (vested minus already withdrawn)
    #[must_use]
    pub fn withdrawable(&self) -> HclawAmount {
        self.vested_amount().saturating_sub(self.withdrawn)
    }

    /// Record a withdrawal
    pub fn withdraw(&mut self, amount: HclawAmount) -> Result<(), VestingError> {
        let available = self.withdrawable();
        if amount > available {
            return Err(VestingError::InsufficientVested {
                available,
                requested: amount,
            });
        }
        self.withdrawn = self.withdrawn.saturating_add(amount);
        Ok(())
    }

    /// Calculate tokens that will be burned (days the verifier missed).
    /// Only meaningful after bootstrap ends.
    #[must_use]
    pub fn forfeited_amount(&self) -> HclawAmount {
        let missed_days = (self.join_day..BOOTSTRAP_DAYS)
            .filter(|&day| !self.daily_active.get(day as usize).copied().unwrap_or(false))
            .count() as u128;

        HclawAmount::from_raw(self.daily_amount.raw() * missed_days)
    }

    /// Whether the schedule is fully vested (all eligible days were active)
    #[must_use]
    pub fn is_fully_vested(&self) -> bool {
        (self.join_day..BOOTSTRAP_DAYS)
            .all(|day| self.daily_active.get(day as usize).copied().unwrap_or(false))
    }
}

/// Vesting errors
#[derive(Debug, thiserror::Error)]
pub enum VestingError {
    /// Not enough tokens have vested to cover the withdrawal
    #[error("insufficient vested: {available} available, {requested} requested")]
    InsufficientVested {
        /// Amount currently available
        available: HclawAmount,
        /// Amount requested
        requested: HclawAmount,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::DAY_MS;

    const START: Timestamp = 0;
    const END: Timestamp = 30 * DAY_MS;

    #[test]
    fn test_immediate_unlock_covers_min_stake() {
        // Tier 3: 3000 tokens, min_stake 3000 — everything immediate
        let schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(3_000),
            HclawAmount::from_hclaw(3_000),
            START,
            END,
            0,
        );

        assert_eq!(schedule.immediate_amount.whole_hclaw(), 3_000);
        assert_eq!(schedule.vesting_amount.whole_hclaw(), 0);
        assert_eq!(schedule.vested_amount().whole_hclaw(), 3_000);
    }

    #[test]
    fn test_daily_vesting_with_liveness() {
        // 100K tokens, 250K min_stake — wait, min_stake > total here.
        // Let's use tier 1: 250K tokens, 250K min_stake — all immediate
        let schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(250_000),
            HclawAmount::from_hclaw(250_000),
            START,
            END,
            0,
        );
        assert_eq!(schedule.vested_amount().whole_hclaw(), 250_000);

        // Hypothetical: 10K tokens, 1K min_stake, joined day 0
        let mut schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(10_000),
            HclawAmount::from_hclaw(1_000),
            START,
            END,
            0,
        );

        // Immediate: 1000, vesting: 9000 over 30 days = 300/day
        assert_eq!(schedule.immediate_amount.whole_hclaw(), 1_000);
        assert_eq!(schedule.daily_amount.whole_hclaw(), 300);

        // No days active yet — only immediate available
        assert_eq!(schedule.vested_amount().whole_hclaw(), 1_000);

        // Active on days 0 and 1
        schedule.mark_day_active(0);
        schedule.mark_day_active(1);
        // 1000 immediate + 300 * 2 = 1600
        assert_eq!(schedule.vested_amount().whole_hclaw(), 1_600);
    }

    #[test]
    fn test_missed_days_dont_vest() {
        let mut schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(10_000),
            HclawAmount::from_hclaw(1_000),
            START,
            END,
            0,
        );

        // Active every other day for 30 days = 15 active days
        for day in (0..30).step_by(2) {
            schedule.mark_day_active(day);
        }

        // 1000 immediate + 300 * 15 = 5500
        assert_eq!(schedule.vested_amount().whole_hclaw(), 5_500);
        assert!(!schedule.is_fully_vested());

        // Forfeited: 300 * 15 = 4500
        assert_eq!(schedule.forfeited_amount().whole_hclaw(), 4_500);
    }

    #[test]
    fn test_late_joiner_fewer_vesting_days() {
        // Join on day 20 — only 10 days of vesting
        let mut schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(10_000),
            HclawAmount::from_hclaw(1_000),
            START,
            END,
            20,
        );

        // Vesting: 9000 / 10 days = 900 per day
        assert_eq!(schedule.daily_amount.whole_hclaw(), 900);

        // Active all 10 days
        for day in 20..30 {
            schedule.mark_day_active(day);
        }

        // 1000 immediate + 900 * 10 = 10000
        assert_eq!(schedule.vested_amount().whole_hclaw(), 10_000);
        assert!(schedule.is_fully_vested());
    }

    #[test]
    fn test_withdrawal() {
        let mut schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(10_000),
            HclawAmount::from_hclaw(1_000),
            START,
            END,
            0,
        );

        schedule.mark_day_active(0);
        // 1000 + 300 = 1300 available
        assert_eq!(schedule.withdrawable().whole_hclaw(), 1_300);

        schedule.withdraw(HclawAmount::from_hclaw(1_000)).unwrap();
        assert_eq!(schedule.withdrawable().whole_hclaw(), 300);

        // Can't over-withdraw
        assert!(schedule.withdraw(HclawAmount::from_hclaw(500)).is_err());
    }

    #[test]
    fn test_tier7_all_immediate() {
        // Tier 7: 100 tokens, min_stake 100 — everything immediate
        let schedule = VestingSchedule::new(
            HclawAmount::from_hclaw(100),
            HclawAmount::from_hclaw(100),
            START,
            END,
            0,
        );

        assert_eq!(schedule.vesting_amount.whole_hclaw(), 0);
        assert_eq!(schedule.vested_amount().whole_hclaw(), 100);
        assert_eq!(schedule.forfeited_amount().whole_hclaw(), 0);
    }
}
