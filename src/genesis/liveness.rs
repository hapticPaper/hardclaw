//! On-chain liveness tracking for vesting qualification.
//!
//! Vesting is gated on daily liveness: a verifier must actually operate
//! their node each day to unlock that day's vesting portion. Liveness is
//! proven by block attestations — the chain already records which verifiers
//! attested to each block. This module aggregates those attestations into
//! daily liveness records.
//!
//! A verifier is considered "active" on a given day if they signed at least
//! `min_attestations_per_day` block attestations during that day's window.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{BOOTSTRAP_DAYS, DAY_MS};
use crate::types::{Address, Timestamp};

/// Minimum attestations per day to count as "active".
/// At 1 block/sec, 86,400 blocks/day — requiring 100 means the verifier
/// must be online and attesting for at least ~100 seconds that day.
pub const MIN_ATTESTATIONS_PER_DAY: u32 = 100;

/// Liveness record for a single day
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyLiveness {
    /// Day number (0-indexed from bootstrap start)
    pub day: u32,
    /// Day start timestamp
    pub day_start: Timestamp,
    /// Day end timestamp
    pub day_end: Timestamp,
    /// Attestation count per verifier address for this day
    pub attestations: HashMap<Address, u32>,
}

impl DailyLiveness {
    /// Create a new daily record
    #[must_use]
    pub fn new(day: u32, bootstrap_start: Timestamp) -> Self {
        let day_start = bootstrap_start + (day as i64 * DAY_MS);
        Self {
            day,
            day_start,
            day_end: day_start + DAY_MS,
            attestations: HashMap::new(),
        }
    }

    /// Record an attestation for a verifier
    pub fn record_attestation(&mut self, verifier: &Address) {
        *self.attestations.entry(*verifier).or_insert(0) += 1;
    }

    /// Check if a verifier met the liveness threshold for this day
    #[must_use]
    pub fn is_active(&self, verifier: &Address, min_attestations: u32) -> bool {
        self.attestations
            .get(verifier)
            .is_some_and(|&count| count >= min_attestations)
    }

    /// Get all verifiers who met liveness for this day
    #[must_use]
    pub fn active_verifiers(&self, min_attestations: u32) -> HashSet<Address> {
        self.attestations
            .iter()
            .filter(|(_, &count)| count >= min_attestations)
            .map(|(&addr, _)| addr)
            .collect()
    }
}

/// Tracks liveness across the full bootstrap period (30 days)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LivenessTracker {
    /// Bootstrap start timestamp (for computing day boundaries)
    bootstrap_start: Timestamp,
    /// Daily records (index = day number)
    days: Vec<DailyLiveness>,
    /// Minimum attestations per day to count as active
    min_attestations: u32,
    /// Current day index (which day are we in)
    current_day: u32,
}

impl LivenessTracker {
    /// Create a new tracker
    #[must_use]
    pub fn new(bootstrap_start: Timestamp) -> Self {
        let mut days = Vec::with_capacity(BOOTSTRAP_DAYS as usize);
        for day in 0..BOOTSTRAP_DAYS {
            days.push(DailyLiveness::new(day, bootstrap_start));
        }

        Self {
            bootstrap_start,
            days,
            min_attestations: MIN_ATTESTATIONS_PER_DAY,
            current_day: 0,
        }
    }

    /// Record an attestation at a given timestamp.
    /// Determines which day the timestamp falls in and records it.
    pub fn record_attestation(&mut self, verifier: &Address, block_timestamp: Timestamp) {
        if let Some(day) = self.day_for_timestamp(block_timestamp) {
            if (day as usize) < self.days.len() {
                self.days[day as usize].record_attestation(verifier);
                if day > self.current_day {
                    self.current_day = day;
                }
            }
        }
    }

    /// Get the day number (0-indexed) for a timestamp
    #[must_use]
    pub fn day_for_timestamp(&self, timestamp: Timestamp) -> Option<u32> {
        if timestamp < self.bootstrap_start {
            return None;
        }
        let day = ((timestamp - self.bootstrap_start) / DAY_MS) as u32;
        if day >= BOOTSTRAP_DAYS {
            None
        } else {
            Some(day)
        }
    }

    /// Count how many days a verifier has been active (met liveness threshold)
    #[must_use]
    pub fn active_days(&self, verifier: &Address) -> u32 {
        self.days
            .iter()
            .filter(|d| d.is_active(verifier, self.min_attestations))
            .count() as u32
    }

    /// Check if a verifier was active on a specific day
    #[must_use]
    pub fn was_active_on_day(&self, verifier: &Address, day: u32) -> bool {
        self.days
            .get(day as usize)
            .is_some_and(|d| d.is_active(verifier, self.min_attestations))
    }

    /// Get the list of days a verifier was active (for vesting calculation)
    #[must_use]
    pub fn active_day_list(&self, verifier: &Address) -> Vec<u32> {
        self.days
            .iter()
            .filter(|d| d.is_active(verifier, self.min_attestations))
            .map(|d| d.day)
            .collect()
    }

    /// Get the current day number
    #[must_use]
    pub fn current_day(&self) -> u32 {
        self.current_day
    }

    /// Get attestation count for a verifier on a specific day
    #[must_use]
    pub fn attestation_count(&self, verifier: &Address, day: u32) -> u32 {
        self.days
            .get(day as usize)
            .and_then(|d| d.attestations.get(verifier))
            .copied()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_addr() -> Address {
        Address::from_public_key(Keypair::generate().public_key())
    }

    #[test]
    fn test_day_calculation() {
        let start = 0i64;
        let tracker = LivenessTracker::new(start);

        assert_eq!(tracker.day_for_timestamp(0), Some(0));
        assert_eq!(tracker.day_for_timestamp(DAY_MS - 1), Some(0));
        assert_eq!(tracker.day_for_timestamp(DAY_MS), Some(1));
        assert_eq!(tracker.day_for_timestamp(29 * DAY_MS), Some(29));
        assert_eq!(tracker.day_for_timestamp(30 * DAY_MS), None); // Past bootstrap
    }

    #[test]
    fn test_attestation_tracking() {
        let start = 0i64;
        let mut tracker = LivenessTracker::new(start);
        let verifier = test_addr();

        // Record 50 attestations on day 0 — not enough
        for _ in 0..50 {
            tracker.record_attestation(&verifier, 1000);
        }
        assert!(!tracker.was_active_on_day(&verifier, 0));

        // Record 50 more — now 100, meets threshold
        for _ in 0..50 {
            tracker.record_attestation(&verifier, 2000);
        }
        assert!(tracker.was_active_on_day(&verifier, 0));
        assert_eq!(tracker.active_days(&verifier), 1);
    }

    #[test]
    fn test_multi_day_liveness() {
        let start = 0i64;
        let mut tracker = LivenessTracker::new(start);
        let verifier = test_addr();

        // Active on days 0, 1, and 5
        for day in [0, 1, 5] {
            let ts = day as i64 * DAY_MS + 1000;
            for _ in 0..MIN_ATTESTATIONS_PER_DAY {
                tracker.record_attestation(&verifier, ts);
            }
        }

        assert_eq!(tracker.active_days(&verifier), 3);
        assert!(tracker.was_active_on_day(&verifier, 0));
        assert!(tracker.was_active_on_day(&verifier, 1));
        assert!(!tracker.was_active_on_day(&verifier, 2));
        assert!(tracker.was_active_on_day(&verifier, 5));

        let days = tracker.active_day_list(&verifier);
        assert_eq!(days, vec![0, 1, 5]);
    }

    #[test]
    fn test_multiple_verifiers_same_day() {
        let start = 0i64;
        let mut tracker = LivenessTracker::new(start);
        let v1 = test_addr();
        let v2 = test_addr();

        // v1 attests 200 times, v2 attests 50 times
        for _ in 0..200 {
            tracker.record_attestation(&v1, 1000);
        }
        for _ in 0..50 {
            tracker.record_attestation(&v2, 1000);
        }

        assert!(tracker.was_active_on_day(&v1, 0));
        assert!(!tracker.was_active_on_day(&v2, 0));
    }
}
