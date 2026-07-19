//! Deterministic multi-chain TFIM execution on a Rayon thread pool.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::{rngs::StdRng, SeedableRng};
use rayon::prelude::*;

use crate::core::Spin;
use crate::sse::{SSEState, SimulationConfig, SimulationResults, SseModel, SseSampler};

/// Configuration for independent TFIM chains sharing one immutable model.
#[derive(Clone, Copy, Debug)]
pub struct ParallelSimulationConfig {
    /// Number of statistically independent chains.
    pub chains: usize,
    /// Number of worker threads in the private Rayon pool.
    pub threads: usize,
    /// Seed from which deterministic per-chain seeds are derived.
    pub master_seed: u64,
    /// Positive inverse temperature shared by all chains.
    pub beta: f64,
    /// Initial padded operator-string cutoff for every chain.
    pub operator_string_length: usize,
    /// Thermalization and measurement counts for each chain.
    pub simulation: SimulationConfig,
}

/// Output and reproducibility metadata for one independent chain.
#[derive(Clone, Copy, Debug)]
pub struct ChainResults {
    /// Stable zero-based chain index.
    pub chain_index: usize,
    /// Deterministically derived random seed used by this chain.
    pub seed: u64,
    /// Simulation result for this chain.
    pub simulation: SimulationResults,
}

/// Energy estimate combined from independent chain means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombinedEnergyResults {
    /// Number of combined chains.
    pub chains: usize,
    /// Total number of expansion-order measurements across chains.
    pub samples: u64,
    /// Unweighted mean of per-chain energy-density estimates.
    pub energy_per_site: f64,
    /// Standard error computed from the variation of independent chain means.
    pub chain_standard_error: f64,
}

/// Per-chain and combined output from [`run_parallel_tfim`].
#[derive(Debug)]
pub struct ParallelSimulationResults {
    /// Chain results sorted by [`ChainResults::chain_index`].
    pub chains: Vec<ChainResults>,
    /// Energy density and uncertainty combined across chains.
    pub combined_energy: CombinedEnergyResults,
    /// Elapsed wall time including thread-pool construction.
    pub wall_time: Duration,
}

/// Failure while validating or executing a parallel simulation.
#[derive(Debug)]
pub enum ParallelSimulationError {
    /// The requested chain count was zero.
    NoChains,
    /// The requested Rayon worker count was zero.
    NoThreads,
    /// The shared initial basis state has the wrong number of spins.
    InvalidInitialStateLength {
        /// Received number of spins.
        received: usize,
        /// Number of sites required by the model.
        expected: usize,
    },
    /// Rayon could not construct the private worker pool.
    ThreadPool(String),
    /// One independent chain failed.
    Chain {
        /// Index of the failed chain.
        chain_index: usize,
        /// Display representation of the underlying error.
        error: String,
    },
}

impl fmt::Display for ParallelSimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoChains => write!(f, "parallel simulation requires at least one chain"),
            Self::NoThreads => write!(f, "parallel simulation requires at least one thread"),
            Self::InvalidInitialStateLength { received, expected } => write!(
                f,
                "initial basis-state length {received} does not match model site count {expected}"
            ),
            Self::ThreadPool(message) => write!(f, "failed to build Rayon thread pool: {message}"),
            Self::Chain { chain_index, error } => {
                write!(f, "chain {chain_index} failed: {error}")
            }
        }
    }
}

impl Error for ParallelSimulationError {}

/// Runs reproducible independent TFIM chains in a private Rayon thread pool.
///
/// Chain seeds depend only on `master_seed` and chain index, so changing
/// `config.threads` does not change sampled trajectories. The returned vector
/// is sorted by chain index even though chains execute in parallel.
///
/// The combined standard error is computed from the variation of independent
/// chain energy-density means, not from individual correlated measurements.
///
/// # Errors
///
/// Returns an error for zero chains/threads, initial-state length mismatch,
/// thread-pool construction failure, or any failed chain.
pub fn run_parallel_tfim<M>(
    model: Arc<M>,
    initial_basis_state: &[Spin],
    config: ParallelSimulationConfig,
) -> Result<ParallelSimulationResults, ParallelSimulationError>
where
    M: SseModel + Send + Sync + 'static,
{
    let started = Instant::now();
    if config.chains == 0 {
        return Err(ParallelSimulationError::NoChains);
    }
    if config.threads == 0 {
        return Err(ParallelSimulationError::NoThreads);
    }
    if initial_basis_state.len() != model.num_sites() {
        return Err(ParallelSimulationError::InvalidInitialStateLength {
            received: initial_basis_state.len(),
            expected: model.num_sites(),
        });
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.threads)
        .build()
        .map_err(|error| ParallelSimulationError::ThreadPool(error.to_string()))?;
    let mut chains = pool.install(|| {
        (0..config.chains)
            .into_par_iter()
            .map(|chain_index| {
                run_chain(Arc::clone(&model), initial_basis_state, config, chain_index)
            })
            .collect::<Result<Vec<_>, _>>()
    })?;
    chains.sort_by_key(|chain| chain.chain_index);

    let combined_energy = combine_energy(&chains);
    Ok(ParallelSimulationResults {
        chains,
        combined_energy,
        wall_time: started.elapsed(),
    })
}

fn run_chain<M>(
    model: Arc<M>,
    initial_basis_state: &[Spin],
    config: ParallelSimulationConfig,
    chain_index: usize,
) -> Result<ChainResults, ParallelSimulationError>
where
    M: SseModel + Send + Sync + 'static,
{
    let seed = derive_chain_seed(config.master_seed, chain_index as u64);
    let state = SSEState::new(
        model.as_ref(),
        initial_basis_state.to_vec(),
        config.operator_string_length,
    )
    .map_err(|error| chain_error(chain_index, error))?;
    let rng = StdRng::seed_from_u64(seed);
    let mut sampler = SseSampler::with_shared_model(model, state, config.beta, rng)
        .map_err(|error| chain_error(chain_index, error))?;
    let simulation = sampler
        .run_tfim(config.simulation)
        .map_err(|error| chain_error(chain_index, error))?;

    Ok(ChainResults {
        chain_index,
        seed,
        simulation,
    })
}

fn chain_error(chain_index: usize, error: impl fmt::Display) -> ParallelSimulationError {
    ParallelSimulationError::Chain {
        chain_index,
        error: error.to_string(),
    }
}

fn combine_energy(chains: &[ChainResults]) -> CombinedEnergyResults {
    let count = chains.len() as f64;
    let mean = chains
        .iter()
        .map(|chain| chain.simulation.thermodynamics.energy_per_site)
        .sum::<f64>()
        / count;
    let variance = if chains.len() > 1 {
        chains
            .iter()
            .map(|chain| {
                let difference = chain.simulation.thermodynamics.energy_per_site - mean;
                difference * difference
            })
            .sum::<f64>()
            / (count - 1.0)
    } else {
        0.0
    };

    CombinedEnergyResults {
        chains: chains.len(),
        samples: chains
            .iter()
            .map(|chain| chain.simulation.thermodynamics.samples)
            .sum(),
        energy_per_site: mean,
        chain_standard_error: (variance / count).sqrt(),
    }
}

/// Derives a deterministic, statistically separated seed for one chain.
///
/// The mapping is independent of worker count and scheduling. It is part of
/// the crate's reproducibility contract and must remain stable within a schema
/// version.
#[must_use]
pub fn derive_chain_seed(master_seed: u64, chain_index: u64) -> u64 {
    let mut value = master_seed ^ chain_index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoundaryCondition, Geometry, LocalSseModel};

    #[test]
    fn chain_results_are_independent_of_rayon_thread_count() {
        let geometry = Geometry::chain(2, BoundaryCondition::Open).unwrap();
        let model = Arc::new(LocalSseModel::tfim(&geometry, &[(0, 1)], 1.0, 0.5).unwrap());
        let base = ParallelSimulationConfig {
            chains: 4,
            threads: 1,
            master_seed: 42,
            beta: 2.0,
            operator_string_length: 32,
            simulation: SimulationConfig {
                thermalization_sweeps: 20,
                measurement_sweeps: 50,
                sweeps_per_measurement: 1,
            },
        };
        let serial = run_parallel_tfim(Arc::clone(&model), &[Spin::Up; 2], base).unwrap();
        let parallel = run_parallel_tfim(
            model,
            &[Spin::Up; 2],
            ParallelSimulationConfig { threads: 3, ..base },
        )
        .unwrap();

        assert_eq!(serial.combined_energy, parallel.combined_energy);
        for (left, right) in serial.chains.iter().zip(&parallel.chains) {
            assert_eq!(left.chain_index, right.chain_index);
            assert_eq!(left.seed, right.seed);
            assert_eq!(
                left.simulation.thermodynamics,
                right.simulation.thermodynamics
            );
        }
    }
}
