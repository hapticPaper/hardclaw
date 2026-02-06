//! Fee distribution between participants.

use crate::types::{Address, HclawAmount};

/// Result of fee distribution
#[derive(Clone, Debug)]
pub struct FeeDistribution {
    /// Amount to solver
    pub solver_amount: HclawAmount,
    /// Solver's address
    pub solver: Address,
    /// Amount to verifier
    pub verifier_amount: HclawAmount,
    /// Verifier's address
    pub verifier: Address,
    /// Amount to burn
    pub burn_amount: HclawAmount,
}

impl FeeDistribution {
    /// Get total distributed (should equal original bounty)
    #[must_use]
    pub fn total(&self) -> HclawAmount {
        self.solver_amount
            .saturating_add(self.verifier_amount)
            .saturating_add(self.burn_amount)
    }
}

/// Distributes fees according to protocol rules
pub struct FeeDistributor {
    /// Solver share percentage
    solver_share: u8,
    /// Verifier share percentage
    verifier_share: u8,
    /// Burn share percentage
    burn_share: u8,
}

impl FeeDistributor {
    /// Create new distributor with specified shares
    ///
    /// # Panics
    /// Panics if shares don't sum to 100
    #[must_use]
    pub fn new(solver_share: u8, verifier_share: u8, burn_share: u8) -> Self {
        assert_eq!(
            solver_share + verifier_share + burn_share,
            100,
            "Shares must sum to 100"
        );

        Self {
            solver_share,
            verifier_share,
            burn_share,
        }
    }

    /// Create with default shares (95/4/1)
    #[must_use]
    pub fn default_shares() -> Self {
        Self::new(95, 4, 1)
    }

    /// Distribute a bounty amount
    #[must_use]
    pub fn distribute(
        &self,
        bounty: HclawAmount,
        solver: Address,
        verifier: Address,
    ) -> FeeDistribution {
        let solver_amount = bounty.percentage(self.solver_share);
        let verifier_amount = bounty.percentage(self.verifier_share);

        // Burn gets the remainder to handle rounding
        let burn_amount = bounty
            .saturating_sub(solver_amount)
            .saturating_sub(verifier_amount);

        FeeDistribution {
            solver_amount,
            solver,
            verifier_amount,
            verifier,
            burn_amount,
        }
    }

    /// Get current shares
    #[must_use]
    pub const fn shares(&self) -> (u8, u8, u8) {
        (self.solver_share, self.verifier_share, self.burn_share)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;

    fn test_address() -> Address {
        let kp = Keypair::generate();
        Address::from_public_key(kp.public_key())
    }

    #[test]
    fn test_distribution_percentages() {
        let distributor = FeeDistributor::default_shares();
        let bounty = HclawAmount::from_hclaw(100);

        let dist = distributor.distribute(bounty, test_address(), test_address());

        assert_eq!(dist.solver_amount.whole_hclaw(), 95);
        assert_eq!(dist.verifier_amount.whole_hclaw(), 4);
        assert_eq!(dist.burn_amount.whole_hclaw(), 1);
    }

    #[test]
    fn test_distribution_total_preserved() {
        let distributor = FeeDistributor::new(50, 30, 20);
        let bounty = HclawAmount::from_hclaw(1000);

        let dist = distributor.distribute(bounty, test_address(), test_address());

        // Total should equal original (minus any rounding dust)
        let total = dist.total();
        assert!(total.raw() <= bounty.raw());
        assert!(total.raw() >= bounty.raw() - 3); // Allow for rounding
    }

    #[test]
    #[should_panic(expected = "Shares must sum to 100")]
    fn test_invalid_shares() {
        let _ = FeeDistributor::new(50, 50, 50); // Sums to 150, not 100
    }
}
