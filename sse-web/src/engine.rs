//! A resumable single-chain SSE engine mirroring the CLI runner exactly.
//!
//! The sweep sequence per unit of work is identical to `src/runner.rs`:
//! grow headroom, one diagonal sweep, one family-selected cluster or
//! world-line sweep. Thermalization runs first; each measurement records
//! the expansion order after `sweeps_per_measurement` sweeps. Because the
//! chain seed derivation and sweep order match the CLI, a completed engine
//! reproduces the CLI's per-chain expansion-order series bit for bit.

use qslib::sse::{
    derive_chain_seed, BasisSseState, ClusterSweepStats, DiagonalSweepStats, LocalSseModel,
    Operator, SseModel, SseSampler, UpdateScheme,
};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::Serialize;

use crate::config::{ModelConfig, RunConfig, RydbergUpdate};
use crate::model::initial_bits;

/// Execution phase reported to the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Discarding pre-measurement sweeps.
    Thermalization,
    /// Recording expansion-order measurements.
    Measurement,
    /// All configured measurements are recorded.
    Complete,
}

/// Incremental result of one `advance` call.
#[derive(Clone, Debug, Serialize)]
pub struct AdvanceReport {
    /// Phase after this batch.
    pub phase: Phase,
    /// Thermalization sweeps still outstanding.
    pub thermalization_remaining: usize,
    /// Measurements still outstanding.
    pub measurements_remaining: usize,
    /// Expansion orders recorded during this batch, in order.
    pub orders: Vec<usize>,
    /// Model energy shift for energy reconstruction.
    pub energy_shift: f64,
    /// Number of lattice sites.
    pub num_sites: usize,
    /// Cumulative diagonal update statistics.
    pub diagonal: DiagonalStats,
    /// Cumulative cluster/world-line update statistics.
    pub clusters: ClusterStats,
}

/// Serializable mirror of the qslib diagonal sweep statistics.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct DiagonalStats {
    /// Proposed insertions.
    pub insertions_proposed: usize,
    /// Accepted insertions.
    pub insertions_accepted: usize,
    /// Proposed removals.
    pub removals_proposed: usize,
    /// Accepted removals.
    pub removals_accepted: usize,
}

/// Serializable mirror of the qslib cluster sweep statistics.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ClusterStats {
    /// Connected components identified.
    pub clusters: usize,
    /// Components flipped.
    pub flipped_clusters: usize,
    /// Partner vertices toggled.
    pub vertices_toggled: usize,
    /// Proposals attempted.
    pub proposals: usize,
    /// Proposals accepted.
    pub proposals_accepted: usize,
}

/// One independent, resumable Monte Carlo chain.
pub struct ChainEngine {
    sampler: SseSampler<LocalSseModel, ChaCha20Rng>,
    scheme: UpdateScheme,
    sweeps_per_measurement: usize,
    thermalization_remaining: usize,
    measurements_remaining: usize,
    sweeps_into_measurement: usize,
    energy_shift: f64,
    num_sites: usize,
    diagonal: DiagonalStats,
    clusters: ClusterStats,
}

impl ChainEngine {
    /// Builds a chain from a validated CLI configuration and chain index.
    ///
    /// Construction order matches the CLI runner: geometry, model, legacy
    /// initial state through the explicit qslib adapter, padded operator
    /// string, then a ChaCha20 stream seeded by `derive_chain_seed`.
    pub fn new(config: &RunConfig, chain_index: u64) -> Result<Self, String> {
        config.validate().map_err(|error| error.to_string())?;
        let geometry = config
            .model
            .geometry()
            .build()
            .map_err(|error| error.to_string())?;
        let model = config
            .model
            .build_model(&geometry)
            .map_err(|error| error.to_string())?;
        let spins = config
            .initial_state
            .build(model.num_sites())
            .map_err(|error| error.to_string())?;
        let bits = initial_bits(config.model.legacy_kind(), &spins);
        let state = BasisSseState::new(
            bits,
            vec![Operator::identity(); config.simulation.operator_string_length],
        )
        .map_err(|error| error.to_string())?;
        let energy_shift = model.energy_shift();
        let num_sites = model.num_sites();
        let sampler = SseSampler::new(
            model,
            state,
            config.simulation.beta,
            ChaCha20Rng::from_seed(derive_chain_seed(config.execution.seed, chain_index)),
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            sampler,
            scheme: update_scheme(&config.model),
            sweeps_per_measurement: config.simulation.sweeps_per_measurement,
            thermalization_remaining: config.simulation.thermalization_sweeps,
            measurements_remaining: config.simulation.measurement_sweeps,
            sweeps_into_measurement: 0,
            energy_shift,
            num_sites,
            diagonal: DiagonalStats::default(),
            clusters: ClusterStats::default(),
        })
    }

    /// Returns the current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        if self.thermalization_remaining > 0 {
            Phase::Thermalization
        } else if self.measurements_remaining > 0 {
            Phase::Measurement
        } else {
            Phase::Complete
        }
    }

    /// Runs up to `budget` sweeps, recording measurements at their exact
    /// configured boundaries, and reports newly recorded expansion orders.
    pub fn advance(&mut self, budget: usize) -> Result<AdvanceReport, String> {
        let mut orders = Vec::new();
        let mut remaining = budget;
        while remaining > 0 {
            if self.thermalization_remaining > 0 {
                self.one_sweep()?;
                self.thermalization_remaining -= 1;
                remaining -= 1;
                continue;
            }
            if self.measurements_remaining == 0 {
                break;
            }
            self.one_sweep()?;
            self.sweeps_into_measurement += 1;
            remaining -= 1;
            if self.sweeps_into_measurement == self.sweeps_per_measurement {
                self.sweeps_into_measurement = 0;
                self.measurements_remaining -= 1;
                orders.push(self.sampler.state().expansion_order());
            }
        }
        Ok(AdvanceReport {
            phase: self.phase(),
            thermalization_remaining: self.thermalization_remaining,
            measurements_remaining: self.measurements_remaining,
            orders,
            energy_shift: self.energy_shift,
            num_sites: self.num_sites,
            diagonal: self.diagonal,
            clusters: self.clusters,
        })
    }

    fn one_sweep(&mut self) -> Result<(), String> {
        self.sampler.ensure_operator_headroom();
        let diagonal = self
            .sampler
            .diagonal_sweep()
            .map_err(|error| error.to_string())?;
        self.diagonal.insertions_proposed += diagonal.insertions_proposed;
        self.diagonal.insertions_accepted += diagonal.insertions_accepted;
        self.diagonal.removals_proposed += diagonal.removals_proposed;
        self.diagonal.removals_accepted += diagonal.removals_accepted;
        let clusters = match self.scheme {
            UpdateScheme::TfimCluster => self.sampler.tfim_cluster_sweep(),
            UpdateScheme::RydbergLocal => self.sampler.rydberg_local_sweep(),
            UpdateScheme::RydbergGlobalReference => self.sampler.rydberg_global_cluster_sweep(),
            UpdateScheme::Local => unreachable!("the web engine never selects the local scheme"),
        }
        .map_err(|error| error.to_string())?;
        self.clusters.clusters += clusters.clusters;
        self.clusters.flipped_clusters += clusters.flipped_clusters;
        self.clusters.vertices_toggled += clusters.vertices_toggled;
        self.clusters.proposals += clusters.proposals;
        self.clusters.proposals_accepted += clusters.proposals_accepted;
        Ok(())
    }
}

fn update_scheme(model: &ModelConfig) -> UpdateScheme {
    match model {
        ModelConfig::Tfim { .. } => UpdateScheme::TfimCluster,
        ModelConfig::Rydberg { update, .. } => match update {
            RydbergUpdate::Local => UpdateScheme::RydbergLocal,
            RydbergUpdate::GlobalReference => UpdateScheme::RydbergGlobalReference,
        },
    }
}

// Keep the unused-import warnings away on non-wasm builds where the sweep
// stats types are only consumed through the mirrors above.
const _: fn(DiagonalSweepStats) = |_| {};
const _: fn(ClusterSweepStats) = |_| {};
