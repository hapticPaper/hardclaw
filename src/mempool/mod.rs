//! Transaction and Job Mempool.
//!
//! Holds pending jobs and solutions waiting to be included in blocks.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::types::{now_millis, Id, JobPacket, SolutionCandidate, Timestamp};

/// Priority-ordered job entry
#[derive(Clone, Debug)]
struct PrioritizedJob {
    job: JobPacket,
    priority: u64,
    added_at: Timestamp,
}

impl PartialEq for PrioritizedJob {
    fn eq(&self, other: &Self) -> bool {
        self.job.id == other.job.id
    }
}

impl Eq for PrioritizedJob {}

impl PartialOrd for PrioritizedJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedJob {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher bounty = higher priority
        // Same bounty = older job first
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.added_at.cmp(&self.added_at))
    }
}

/// Mempool for pending jobs and solutions
pub struct Mempool {
    /// Pending jobs by ID
    jobs: HashMap<Id, JobPacket>,
    /// Priority queue for job selection
    job_queue: BinaryHeap<PrioritizedJob>,
    /// Solutions by ID
    solutions: HashMap<Id, SolutionCandidate>,
    /// Solutions indexed by job ID
    solutions_by_job: HashMap<Id, Vec<Id>>,
    /// Maximum jobs in mempool
    max_jobs: usize,
    /// Maximum solutions in mempool
    max_solutions: usize,
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}

impl Mempool {
    /// Default max jobs
    pub const DEFAULT_MAX_JOBS: usize = 10_000;
    /// Default max solutions
    pub const DEFAULT_MAX_SOLUTIONS: usize = 50_000;

    /// Create new mempool
    #[must_use]
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            job_queue: BinaryHeap::new(),
            solutions: HashMap::new(),
            solutions_by_job: HashMap::new(),
            max_jobs: Self::DEFAULT_MAX_JOBS,
            max_solutions: Self::DEFAULT_MAX_SOLUTIONS,
        }
    }

    /// Add a job to the mempool
    pub fn add_job(&mut self, job: JobPacket) -> Result<(), MempoolError> {
        // Check if already exists
        if self.jobs.contains_key(&job.id) {
            return Err(MempoolError::DuplicateJob);
        }

        // Check capacity
        if self.jobs.len() >= self.max_jobs {
            return Err(MempoolError::Full);
        }

        // Validate job
        if job.is_expired() {
            return Err(MempoolError::Expired);
        }

        // Calculate priority (bounty-based, using whole units to avoid overflow)
        let priority = job.bounty.whole_hclaw();

        let prioritized = PrioritizedJob {
            job: job.clone(),
            priority,
            added_at: now_millis(),
        };

        self.jobs.insert(job.id, job);
        self.job_queue.push(prioritized);

        Ok(())
    }

    /// Add a solution to the mempool
    pub fn add_solution(&mut self, solution: SolutionCandidate) -> Result<(), MempoolError> {
        // Check if job exists
        if !self.jobs.contains_key(&solution.job_id) {
            return Err(MempoolError::JobNotFound);
        }

        // Check if solution already exists
        if self.solutions.contains_key(&solution.id) {
            return Err(MempoolError::DuplicateSolution);
        }

        // Check capacity
        if self.solutions.len() >= self.max_solutions {
            return Err(MempoolError::Full);
        }

        // Index by job
        self.solutions_by_job
            .entry(solution.job_id)
            .or_default()
            .push(solution.id);

        self.solutions.insert(solution.id, solution);

        Ok(())
    }

    /// Get a job by ID
    #[must_use]
    pub fn get_job(&self, id: &Id) -> Option<&JobPacket> {
        self.jobs.get(id)
    }

    /// Get a solution by ID
    #[must_use]
    pub fn get_solution(&self, id: &Id) -> Option<&SolutionCandidate> {
        self.solutions.get(id)
    }

    /// Get solutions for a job
    #[must_use]
    pub fn solutions_for_job(&self, job_id: &Id) -> Vec<&SolutionCandidate> {
        self.solutions_by_job
            .get(job_id)
            .map(|ids| ids.iter().filter_map(|id| self.solutions.get(id)).collect())
            .unwrap_or_default()
    }

    /// Pop the highest priority job
    pub fn pop_job(&mut self) -> Option<JobPacket> {
        while let Some(prioritized) = self.job_queue.pop() {
            // Skip if job was removed or expired
            if let Some(job) = self.jobs.remove(&prioritized.job.id) {
                if !job.is_expired() {
                    return Some(job);
                }
            }
        }
        None
    }

    /// Pop pending solutions for verification
    pub fn pop_solutions(&mut self, limit: usize) -> Vec<(JobPacket, SolutionCandidate)> {
        let mut results = Vec::new();

        let job_ids: Vec<Id> = self.solutions_by_job.keys().copied().collect();

        for job_id in job_ids {
            if results.len() >= limit {
                break;
            }

            if let Some(job) = self.jobs.get(&job_id).cloned() {
                if let Some(solution_ids) = self.solutions_by_job.get_mut(&job_id) {
                    while let Some(sol_id) = solution_ids.pop() {
                        if let Some(solution) = self.solutions.remove(&sol_id) {
                            results.push((job.clone(), solution));

                            if results.len() >= limit {
                                break;
                            }
                        }
                    }
                }
            }
        }

        results
    }

    /// Remove a job and its solutions
    pub fn remove_job(&mut self, id: &Id) {
        self.jobs.remove(id);

        if let Some(solution_ids) = self.solutions_by_job.remove(id) {
            for sol_id in solution_ids {
                self.solutions.remove(&sol_id);
            }
        }
    }

    /// Remove expired jobs
    pub fn cleanup_expired(&mut self) {
        let expired: Vec<Id> = self
            .jobs
            .iter()
            .filter(|(_, job)| job.is_expired())
            .map(|(id, _)| *id)
            .collect();

        for id in expired {
            self.remove_job(&id);
        }
    }

    /// Get mempool size
    #[must_use]
    pub fn size(&self) -> MempoolSize {
        MempoolSize {
            jobs: self.jobs.len(),
            solutions: self.solutions.len(),
        }
    }
}

/// Mempool size information
#[derive(Clone, Debug)]
pub struct MempoolSize {
    /// Number of jobs
    pub jobs: usize,
    /// Number of solutions
    pub solutions: usize,
}

/// Mempool errors
#[derive(Debug, thiserror::Error)]
pub enum MempoolError {
    /// Duplicate job
    #[error("job already exists")]
    DuplicateJob,
    /// Duplicate solution
    #[error("solution already exists")]
    DuplicateSolution,
    /// Job not found
    #[error("job not found")]
    JobNotFound,
    /// Mempool is full
    #[error("mempool is full")]
    Full,
    /// Job expired
    #[error("job has expired")]
    Expired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{Hash, Keypair};
    use crate::types::{HclawAmount, JobType, VerificationSpec};

    fn create_test_job(bounty: u64) -> JobPacket {
        let kp = Keypair::generate();
        JobPacket::new(
            JobType::Deterministic,
            *kp.public_key(),
            b"input".to_vec(),
            "Test".to_string(),
            HclawAmount::from_hclaw(bounty),
            HclawAmount::from_hclaw(1),
            VerificationSpec::HashMatch {
                expected_hash: Hash::ZERO,
            },
            3600,
        )
    }

    fn create_test_solution(job_id: Id) -> SolutionCandidate {
        let kp = Keypair::generate();
        SolutionCandidate::new(job_id, *kp.public_key(), b"output".to_vec())
    }

    #[test]
    fn test_add_job() {
        let mut mempool = Mempool::new();
        let job = create_test_job(100);

        assert!(mempool.add_job(job.clone()).is_ok());
        assert!(mempool.get_job(&job.id).is_some());
    }

    #[test]
    fn test_priority_ordering() {
        let mut mempool = Mempool::new();

        let low_bounty = create_test_job(10);
        let high_bounty = create_test_job(100);

        mempool.add_job(low_bounty).unwrap();
        mempool.add_job(high_bounty).unwrap();

        // High bounty should come first
        let popped = mempool.pop_job().unwrap();
        assert_eq!(popped.bounty.whole_hclaw(), 100);
    }

    #[test]
    fn test_solutions_for_job() {
        let mut mempool = Mempool::new();
        let job = create_test_job(100);

        mempool.add_job(job.clone()).unwrap();

        let sol1 = create_test_solution(job.id);
        let sol2 = create_test_solution(job.id);

        mempool.add_solution(sol1).unwrap();
        mempool.add_solution(sol2).unwrap();

        let solutions = mempool.solutions_for_job(&job.id);
        assert_eq!(solutions.len(), 2);
    }
}
