//! Elastic token supply management.
//!
//! HCLAW supply is elastic: as compute demand rises, mining difficulty adjusts,
//! and minting equates to resource availability.

use crate::types::HclawAmount;

/// Supply metrics
#[derive(Clone, Debug, Default)]
pub struct SupplyMetrics {
    /// Total tokens minted
    pub total_minted: HclawAmount,
    /// Total tokens burned
    pub total_burned: HclawAmount,
    /// Current circulating supply
    pub circulating_supply: HclawAmount,
    /// Total staked (locked)
    pub total_staked: HclawAmount,
    /// Effective circulating (not staked)
    pub effective_circulating: HclawAmount,
}

impl SupplyMetrics {
    /// Calculate effective circulating supply
    #[must_use]
    pub fn calculate_effective(&self) -> HclawAmount {
        self.circulating_supply.saturating_sub(self.total_staked)
    }

    /// Calculate net supply (minted - burned)
    #[must_use]
    pub fn net_supply(&self) -> HclawAmount {
        self.total_minted.saturating_sub(self.total_burned)
    }

    /// Calculate burn rate (burned / minted as percentage * 100)
    #[must_use]
    pub fn burn_rate(&self) -> f64 {
        if self.total_minted.is_zero() {
            return 0.0;
        }

        self.total_burned.raw() as f64 / self.total_minted.raw() as f64 * 100.0
    }

    /// Calculate stake rate (staked / circulating as percentage)
    #[must_use]
    pub fn stake_rate(&self) -> f64 {
        if self.circulating_supply.is_zero() {
            return 0.0;
        }

        self.total_staked.raw() as f64 / self.circulating_supply.raw() as f64 * 100.0
    }
}

/// Manages token supply and difficulty adjustment
pub struct SupplyManager {
    /// Current metrics
    metrics: SupplyMetrics,
    /// Current difficulty
    difficulty: u64,
    /// Target block time (milliseconds)
    target_block_time_ms: u64,
    /// Blocks to average for difficulty adjustment
    adjustment_window: u64,
    /// Recent block times for adjustment
    recent_block_times: Vec<u64>,
}

impl Default for SupplyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SupplyManager {
    /// Default target block time (1 second)
    pub const DEFAULT_TARGET_BLOCK_TIME_MS: u64 = 1000;
    /// Default adjustment window (100 blocks)
    pub const DEFAULT_ADJUSTMENT_WINDOW: u64 = 100;

    /// Create new supply manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            metrics: SupplyMetrics::default(),
            difficulty: 1,
            target_block_time_ms: Self::DEFAULT_TARGET_BLOCK_TIME_MS,
            adjustment_window: Self::DEFAULT_ADJUSTMENT_WINDOW,
            recent_block_times: Vec::new(),
        }
    }

    /// Record tokens minted (block reward)
    pub fn record_mint(&mut self, amount: HclawAmount) {
        self.metrics.total_minted = self.metrics.total_minted.saturating_add(amount);
        self.metrics.circulating_supply = self.metrics.circulating_supply.saturating_add(amount);
        self.update_effective();
    }

    /// Record tokens minted via airdrop during bootstrap
    pub fn record_airdrop_mint(&mut self, amount: HclawAmount) {
        self.metrics.total_minted = self.metrics.total_minted.saturating_add(amount);
        self.metrics.circulating_supply = self.metrics.circulating_supply.saturating_add(amount);
        self.update_effective();
    }

    /// Record tokens burned
    pub fn record_burn(&mut self, amount: HclawAmount) {
        self.metrics.total_burned = self.metrics.total_burned.saturating_add(amount);
        self.metrics.circulating_supply = self.metrics.circulating_supply.saturating_sub(amount);
        self.update_effective();
    }

    /// Record stake change
    pub fn record_stake_change(&mut self, staked: HclawAmount, unstaked: HclawAmount) {
        self.metrics.total_staked = self
            .metrics
            .total_staked
            .saturating_add(staked)
            .saturating_sub(unstaked);
        self.update_effective();
    }

    fn update_effective(&mut self) {
        self.metrics.effective_circulating = self.metrics.calculate_effective();
    }

    /// Record a block time for difficulty adjustment
    pub fn record_block_time(&mut self, block_time_ms: u64) {
        self.recent_block_times.push(block_time_ms);

        // Keep only the adjustment window
        if self.recent_block_times.len() > self.adjustment_window as usize {
            self.recent_block_times.remove(0);
        }

        // Adjust difficulty if we have enough data
        if self.recent_block_times.len() >= self.adjustment_window as usize {
            self.adjust_difficulty();
        }
    }

    /// Adjust difficulty based on recent block times
    fn adjust_difficulty(&mut self) {
        if self.recent_block_times.is_empty() {
            return;
        }

        let avg_block_time: u64 =
            self.recent_block_times.iter().sum::<u64>() / self.recent_block_times.len() as u64;

        // If blocks are too fast, increase difficulty
        // If blocks are too slow, decrease difficulty
        if avg_block_time < self.target_block_time_ms * 9 / 10 {
            // More than 10% too fast
            self.difficulty = self.difficulty.saturating_add(1);
        } else if avg_block_time > self.target_block_time_ms * 11 / 10 {
            // More than 10% too slow
            self.difficulty = self.difficulty.saturating_sub(1).max(1);
        }
    }

    /// Get current metrics
    #[must_use]
    pub const fn metrics(&self) -> &SupplyMetrics {
        &self.metrics
    }

    /// Get current difficulty
    #[must_use]
    pub const fn difficulty(&self) -> u64 {
        self.difficulty
    }

    /// Get average block time
    #[must_use]
    pub fn average_block_time(&self) -> Option<u64> {
        if self.recent_block_times.is_empty() {
            return None;
        }

        Some(self.recent_block_times.iter().sum::<u64>() / self.recent_block_times.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supply_tracking() {
        let mut manager = SupplyManager::new();

        manager.record_mint(HclawAmount::from_hclaw(1000));
        manager.record_burn(HclawAmount::from_hclaw(100));

        let metrics = manager.metrics();
        assert_eq!(metrics.total_minted.whole_hclaw(), 1000);
        assert_eq!(metrics.total_burned.whole_hclaw(), 100);
        assert_eq!(metrics.circulating_supply.whole_hclaw(), 900);
    }

    #[test]
    fn test_stake_tracking() {
        let mut manager = SupplyManager::new();

        manager.record_mint(HclawAmount::from_hclaw(1000));
        manager.record_stake_change(HclawAmount::from_hclaw(500), HclawAmount::ZERO);

        let metrics = manager.metrics();
        assert_eq!(metrics.total_staked.whole_hclaw(), 500);
        assert_eq!(metrics.effective_circulating.whole_hclaw(), 500);
    }

    #[test]
    fn test_difficulty_adjustment_fast_blocks() {
        let mut manager = SupplyManager::new();
        manager.adjustment_window = 10;

        // Simulate fast blocks (500ms instead of 1000ms)
        for _ in 0..15 {
            manager.record_block_time(500);
        }

        // Difficulty should have increased
        assert!(manager.difficulty() > 1);
    }

    #[test]
    fn test_difficulty_adjustment_slow_blocks() {
        let mut manager = SupplyManager::new();
        manager.adjustment_window = 10;
        manager.difficulty = 10; // Start higher

        // Simulate slow blocks (1500ms instead of 1000ms)
        for _ in 0..15 {
            manager.record_block_time(1500);
        }

        // Difficulty should have decreased
        assert!(manager.difficulty() < 10);
    }

    #[test]
    fn test_burn_rate() {
        let mut manager = SupplyManager::new();

        manager.record_mint(HclawAmount::from_hclaw(1000));
        manager.record_burn(HclawAmount::from_hclaw(100));

        let rate = manager.metrics().burn_rate();
        assert!((rate - 10.0).abs() < 0.01); // 10%
    }
}
