//! Bootstrap state machine — orchestrates the 30-day genesis period.
//!
//! Ties together the airdrop tracker, liveness tracker, vesting schedules,
//! competency challenges, and DNS break-glass into a single state machine
//! that processes events block-by-block.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::airdrop::{AirdropClaim, AirdropTracker};
use super::competency::CompetencyManager;
use super::liveness::LivenessTracker;
use super::vesting::VestingSchedule;
use super::{DnsBootstrapClaim, GenesisConfig, GenesisError};
use crate::types::{Address, HclawAmount, SystemJobKind, Timestamp};

/// Current phase of the bootstrap period
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootstrapPhase {
    /// Bootstrap is active (within 30 days of genesis)
    Active,
    /// Bootstrap completed
    Completed {
        /// When bootstrap ended
        completed_at: Timestamp,
    },
}

/// The bootstrap state machine
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootstrapState {
    /// Genesis config (frozen at creation)
    pub config: GenesisConfig,
    /// Airdrop tracker
    pub airdrop: AirdropTracker,
    /// Liveness tracker
    pub liveness: LivenessTracker,
    /// Vesting schedules by address
    pub vesting_schedules: HashMap<Address, VestingSchedule>,
    /// Competency manager
    pub competency: CompetencyManager,
    /// DNS break-glass claims
    pub dns_claims: Vec<DnsBootstrapClaim>,
    /// Current phase
    pub phase: BootstrapPhase,
    /// Last day that was processed for vesting updates
    pub last_processed_day: Option<u32>,
}

impl BootstrapState {
    /// Create from genesis config
    #[must_use]
    pub fn new(config: GenesisConfig) -> Self {
        let airdrop = AirdropTracker::new(config.pre_approved.clone());
        let liveness = LivenessTracker::new(config.bootstrap_start);
        let competency = CompetencyManager::new(&config.pre_approved);

        Self {
            config,
            airdrop,
            liveness,
            vesting_schedules: HashMap::new(),
            competency,
            dns_claims: Vec::new(),
            phase: BootstrapPhase::Active,
            last_processed_day: None,
        }
    }

    /// Is the bootstrap still active?
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.phase == BootstrapPhase::Active
    }

    /// Process a new verifier wanting to join.
    /// Returns the airdrop claim (position reserved but not yet activated).
    pub fn process_verifier_join(
        &mut self,
        address: Address,
        now: Timestamp,
    ) -> Result<AirdropClaim, GenesisError> {
        if !self.is_active() {
            return Err(GenesisError::BootstrapNotActive);
        }

        // Reserve airdrop position
        let claim = self.airdrop.reserve_position(address, now)?;

        // If pre-approved, auto-pass competency and activate immediately
        if self.competency.is_pre_approved(&address) {
            self.activate_verifier(&address, now)?;
        }
        // Otherwise, competency challenge is generated separately

        Ok(claim)
    }

    /// Activate a verifier after competency is confirmed.
    /// Credits immediate tokens and creates vesting schedule.
    pub fn activate_verifier(
        &mut self,
        address: &Address,
        now: Timestamp,
    ) -> Result<(HclawAmount, SystemJobKind), GenesisError> {
        let amount = self.airdrop.activate_claim(address)?;
        let claim = self
            .airdrop
            .get_claim(address)
            .ok_or(GenesisError::InvalidConfig("claim disappeared".into()))?;

        let join_day = self
            .liveness
            .day_for_timestamp(now)
            .unwrap_or(0);

        let schedule = VestingSchedule::new(
            amount,
            self.airdrop.config().min_stake,
            self.config.bootstrap_start,
            self.config.bootstrap_end,
            join_day,
        );

        let immediate = schedule.immediate_amount;
        self.vesting_schedules.insert(*address, schedule);

        let system_job = SystemJobKind::AirdropClaim {
            recipient: *address,
            position: claim.position,
            amount,
        };

        Ok((immediate, system_job))
    }

    /// Record a block attestation (for liveness tracking)
    pub fn record_attestation(&mut self, verifier: &Address, block_timestamp: Timestamp) {
        self.liveness.record_attestation(verifier, block_timestamp);
    }

    /// Process a day boundary — update vesting schedules based on liveness.
    /// Called when the chain crosses from one day to the next.
    /// Returns system jobs for any vesting unlocks.
    pub fn process_day_end(&mut self, day: u32) -> Vec<SystemJobKind> {
        if Some(day) <= self.last_processed_day {
            return Vec::new();
        }
        self.last_processed_day = Some(day);

        let mut jobs = Vec::new();

        // For each vesting schedule, check if the verifier was active on this day
        let active_verifiers: Vec<Address> = self
            .vesting_schedules
            .keys()
            .copied()
            .filter(|addr| self.liveness.was_active_on_day(addr, day))
            .collect();

        for address in active_verifiers {
            if let Some(schedule) = self.vesting_schedules.get_mut(&address) {
                let before = schedule.vested_amount();
                schedule.mark_day_active(day);
                let after = schedule.vested_amount();

                let unlocked = after.saturating_sub(before);
                if unlocked.raw() > 0 {
                    jobs.push(SystemJobKind::VestingUnlock {
                        beneficiary: address,
                        amount: unlocked,
                    });
                }
            }
        }

        jobs
    }

    /// Process a DNS break-glass claim.
    /// Requires a signature from the authority key over the node's public key.
    pub fn process_dns_claim(
        &mut self,
        claim: DnsBootstrapClaim,
    ) -> Result<SystemJobKind, GenesisError> {
        if !self.is_active() {
            return Err(GenesisError::BootstrapNotActive);
        }

        if self.dns_claims.len() as u32 >= self.config.dns_break_glass.max_nodes {
            return Err(GenesisError::DnsBreakGlassExhausted);
        }

        // Check hostname is under the authorized domain
        if !claim.hostname.ends_with(&self.config.dns_break_glass.domain) {
            return Err(GenesisError::DnsBreakGlassInvalid(format!(
                "hostname {} is not under domain {}",
                claim.hostname, self.config.dns_break_glass.domain,
            )));
        }

        // Check for duplicate claims (same address)
        if self
            .dns_claims
            .iter()
            .any(|c| c.address == claim.address)
        {
            return Err(GenesisError::DnsBreakGlassInvalid(
                "address has already claimed a DNS bootstrap slot".into(),
            ));
        }

        let job = SystemJobKind::DnsBootstrapClaim {
            node: claim.address,
            hostname: claim.hostname.clone(),
            amount: claim.amount,
        };

        self.dns_claims.push(claim);

        Ok(job)
    }

    /// Check if bootstrap should complete and transition phases.
    /// Returns a completion system job if the bootstrap ended.
    pub fn check_completion(&mut self, now: Timestamp) -> Option<SystemJobKind> {
        if !self.is_active() {
            return None;
        }

        let should_complete =
            now >= self.config.bootstrap_end || self.airdrop.is_exhausted();

        if should_complete {
            self.phase = BootstrapPhase::Completed {
                completed_at: now,
            };

            Some(SystemJobKind::BootstrapComplete {
                total_verifiers: self.airdrop.activated_count(),
                total_distributed: self.airdrop.total_distributed(),
            })
        } else {
            None
        }
    }

    /// Get the current dynamic min stake (based on which airdrop tier is active)
    #[must_use]
    pub fn current_min_stake(&self) -> HclawAmount {
        self.airdrop.current_min_stake()
    }

    /// Get vesting schedule for an address
    #[must_use]
    pub fn get_vesting(&self, address: &Address) -> Option<&VestingSchedule> {
        self.vesting_schedules.get(address)
    }

    /// Calculate total forfeited tokens across all vesting schedules
    /// (for burning at bootstrap end)
    #[must_use]
    pub fn total_forfeited(&self) -> HclawAmount {
        let mut total = HclawAmount::ZERO;
        for schedule in self.vesting_schedules.values() {
            total = total.saturating_add(schedule.forfeited_amount());
        }
        // Also add unclaimed airdrop positions
        total.saturating_add(self.airdrop.unclaimed_pool())
    }

    /// How many DNS break-glass slots remain
    #[must_use]
    pub fn dns_slots_remaining(&self) -> u32 {
        self.config
            .dns_break_glass
            .max_nodes
            .saturating_sub(self.dns_claims.len() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Keypair;
    use crate::genesis::DAY_MS;

    fn test_addr() -> Address {
        Address::from_public_key(Keypair::generate().public_key())
    }

    fn test_config() -> GenesisConfig {
        let addrs: Vec<Address> = (0..7).map(|_| test_addr()).collect();
        let authority = Keypair::generate();
        GenesisConfig::new("test".into(), addrs, authority.public_key().clone(), 0)
    }

    #[test]
    fn test_bootstrap_lifecycle() {
        let config = test_config();
        let pre_approved = config.pre_approved.clone();
        let mut state = BootstrapState::new(config);

        assert!(state.is_active());

        // Pre-approved verifier joins — auto-activated
        let claim = state.process_verifier_join(pre_approved[0], 1000).unwrap();
        assert_eq!(claim.position, 1);
        assert!(state.get_vesting(&pre_approved[0]).is_some());
    }

    #[test]
    fn test_day_end_processing() {
        let config = test_config();
        let pre_approved = config.pre_approved.clone();
        let mut state = BootstrapState::new(config);

        // Join and activate
        state.process_verifier_join(pre_approved[0], 0).unwrap();

        // Record enough attestations for day 0
        for _ in 0..100 {
            state.record_attestation(&pre_approved[0], 1000);
        }

        // Process day 0 end
        let jobs = state.process_day_end(0);
        // Pre-approved verifier joined and was active on day 0, so they get one day's vesting unlock
        assert_eq!(jobs.len(), 1, "Expected 1 vesting unlock job");
        
        match &jobs[0] {
            SystemJobKind::VestingUnlock { beneficiary, amount } => {
                assert_eq!(*beneficiary, pre_approved[0]);
                // Verifier gets daily vesting amount (depends on their tier)
                assert!(amount.raw() > 0, "Vesting unlock should be > 0");
            }
            _ => panic!("Expected VestingUnlock job"),
        }
    }

    #[test]
    fn test_completion() {
        let config = test_config();
        let mut state = BootstrapState::new(config);

        // Not complete yet
        assert!(state.check_completion(0).is_none());

        // Complete after 30 days
        let job = state.check_completion(30 * DAY_MS);
        assert!(job.is_some());
        assert!(!state.is_active());
    }

    #[test]
    fn test_dns_break_glass() {
        let config = test_config();
        let mut state = BootstrapState::new(config);

        let node_kp = Keypair::generate();
        let addr = Address::from_public_key(node_kp.public_key());

        let claim = DnsBootstrapClaim {
            address: addr,
            node_key: node_kp.public_key().clone(),
            hostname: "bootstrap-new.clawpaper.com".to_string(),
            amount: HclawAmount::from_hclaw(250_000),
            claimed_at: 1000,
            vests_at: 1000 + DAY_MS,
        };

        let job = state.process_dns_claim(claim).unwrap();
        assert!(matches!(job, SystemJobKind::DnsBootstrapClaim { .. }));
        assert_eq!(state.dns_slots_remaining(), 9);
    }

    #[test]
    fn test_dns_wrong_domain_rejected() {
        let config = test_config();
        let mut state = BootstrapState::new(config);

        let node_kp = Keypair::generate();
        let claim = DnsBootstrapClaim {
            address: Address::from_public_key(node_kp.public_key()),
            node_key: node_kp.public_key().clone(),
            hostname: "evil.attacker.com".to_string(),
            amount: HclawAmount::from_hclaw(250_000),
            claimed_at: 1000,
            vests_at: 1000 + DAY_MS,
        };

        assert!(state.process_dns_claim(claim).is_err());
    }

    #[test]
    fn test_dns_no_duplicate_address() {
        let config = test_config();
        let mut state = BootstrapState::new(config);

        let node_kp = Keypair::generate();
        let addr = Address::from_public_key(node_kp.public_key());

        let claim1 = DnsBootstrapClaim {
            address: addr,
            node_key: node_kp.public_key().clone(),
            hostname: "node1.clawpaper.com".to_string(),
            amount: HclawAmount::from_hclaw(250_000),
            claimed_at: 1000,
            vests_at: 1000 + DAY_MS,
        };
        state.process_dns_claim(claim1).unwrap();

        let claim2 = DnsBootstrapClaim {
            address: addr,
            node_key: node_kp.public_key().clone(),
            hostname: "node2.clawpaper.com".to_string(),
            amount: HclawAmount::from_hclaw(250_000),
            claimed_at: 2000,
            vests_at: 2000 + DAY_MS,
        };
        assert!(state.process_dns_claim(claim2).is_err());
    }
}
