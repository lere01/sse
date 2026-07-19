//! Monte Carlo updates and run orchestration for fixed-length SSE states.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::Rng;

use crate::core::OperatorKind;
use crate::sse::{
    Operator, SSEState, SseModel, SseModelError, SseTermSites, ThermodynamicAccumulator,
    ThermodynamicResults,
};

/// Failure while constructing or advancing an [`SseSampler`].
#[derive(Debug)]
pub enum SamplerError {
    /// Inverse temperature was non-positive or non-finite.
    InvalidBeta(f64),
    /// The model cannot participate in diagonal insertion/removal updates.
    NoDiagonalTerms,
    /// A TFIM cluster update was requested for an incompatible model.
    UnsupportedClusterUpdate,
    /// A run configuration violated a sampler invariant.
    InvalidSimulationConfig(&'static str),
    /// Model validation, matrix-element, or propagation failure.
    Model(SseModelError),
}

impl fmt::Display for SamplerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBeta(beta) => write!(f, "beta must be finite and positive; got {beta}"),
            Self::NoDiagonalTerms => write!(f, "the SSE model has no diagonal terms"),
            Self::UnsupportedClusterUpdate => {
                write!(f, "the model does not support the TFIM cluster update")
            }
            Self::InvalidSimulationConfig(message) => {
                write!(f, "invalid simulation configuration: {message}")
            }
            Self::Model(error) => error.fmt(f),
        }
    }
}

impl Error for SamplerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Model(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SseModelError> for SamplerError {
    fn from(error: SseModelError) -> Self {
        Self::Model(error)
    }
}

/// Proposal and acceptance counts from diagonal insertion/removal sweeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiagonalSweepStats {
    /// Identity positions considered for insertion.
    pub insertions_proposed: usize,
    /// Proposed insertions accepted by the Metropolis rule.
    pub insertions_accepted: usize,
    /// Diagonal operators considered for removal.
    pub removals_proposed: usize,
    /// Proposed removals accepted by the Metropolis rule.
    pub removals_accepted: usize,
}

/// Structural and acceptance counts from cluster or world-line updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterSweepStats {
    /// Number of connected components identified by a cluster update.
    pub clusters: usize,
    /// Number of components or whole world lines selected for flipping.
    pub flipped_clusters: usize,
    /// Number of diagonal/off-diagonal partner vertices toggled.
    pub vertices_toggled: usize,
    /// Number of Metropolis-corrected proposals attempted.
    pub proposals: usize,
    /// Number of corrected proposals accepted.
    pub proposals_accepted: usize,
}

/// Sweep counts controlling thermalization and measurement.
#[derive(Clone, Copy, Debug)]
pub struct SimulationConfig {
    /// Complete update sweeps discarded before measurement.
    pub thermalization_sweeps: usize,
    /// Number of expansion-order measurements to accumulate.
    pub measurement_sweeps: usize,
    /// Complete update sweeps performed between consecutive measurements.
    pub sweeps_per_measurement: usize,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            thermalization_sweeps: 1_000,
            measurement_sweeps: 10_000,
            sweeps_per_measurement: 1,
        }
    }
}

/// Thermodynamics, aggregate update counts, and timing from one run.
#[derive(Clone, Copy, Debug)]
pub struct SimulationResults {
    /// Expansion-order thermodynamic estimators.
    pub thermodynamics: ThermodynamicResults,
    /// Aggregate diagonal update statistics from the measurement phase.
    pub diagonal: DiagonalSweepStats,
    /// Aggregate cluster/world-line statistics from the measurement phase.
    pub clusters: ClusterSweepStats,
    /// Wall-clock breakdown for the complete run.
    pub timing: RunTiming,
}

/// One recorded expansion-order sample from a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasurementRecord {
    /// Zero-based position in the measurement series.
    pub measurement_index: usize,
    /// Number of non-identity operators at this measurement.
    pub expansion_order: usize,
}

/// Aggregate results together with the underlying expansion-order series.
///
/// Recording is optional because library callers interested only in aggregate
/// thermodynamics should not pay the memory cost of retaining every sample.
#[derive(Debug)]
pub struct RecordedSimulationResults {
    /// Aggregate thermodynamics, update statistics, and timing.
    pub simulation: SimulationResults,
    /// Expansion-order measurements in sampling order.
    pub measurements: Vec<MeasurementRecord>,
}

/// Wall-clock and work-count breakdown for a simulation run.
///
/// Update durations include both thermalization and measurement phases. The
/// phase durations are wall times and therefore are not expected to equal the
/// sum of individual categories exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunTiming {
    /// Total elapsed run time.
    pub total: Duration,
    /// Elapsed thermalization phase time.
    pub thermalization: Duration,
    /// Elapsed measurement phase time.
    pub measurement: Duration,
    /// Time spent in diagonal insertion/removal updates.
    pub diagonal_updates: Duration,
    /// Time spent in cluster or world-line updates.
    pub cluster_updates: Duration,
    /// Time spent recording expansion-order measurements.
    pub accumulation: Duration,
    /// Number of complete update sweeps during thermalization.
    pub thermalization_sweeps: u64,
    /// Number of complete update sweeps during the measurement phase.
    pub measurement_sweeps: u64,
    /// Number of accumulated measurements.
    pub measurements: u64,
}

/// Fixed-length stochastic-series-expansion sampler over a generic model.
///
/// The sampler owns its random-number generator and [`SSEState`]. The model is
/// reference-counted so independent chains can share immutable local terms.
/// The operator string grows automatically when headroom becomes small.
pub struct SseSampler<M, R> {
    model: Arc<M>,
    state: SSEState,
    beta: f64,
    rng: R,
}

impl<M: SseModel, R: Rng> SseSampler<M, R> {
    /// Creates a sampler that takes ownership of a model.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid inverse temperature, a model without
    /// diagonal terms, or an initial state whose operator string does not close
    /// the trace.
    pub fn new(model: M, state: SSEState, beta: f64, rng: R) -> Result<Self, SamplerError> {
        Self::with_shared_model(Arc::new(model), state, beta, rng)
    }

    /// Creates a sampler using an existing shared model allocation.
    ///
    /// This constructor is useful for independent chains. Each sampler still
    /// owns a separate state and random-number generator.
    ///
    /// # Errors
    ///
    /// Returns the same validation errors as [`SseSampler::new`].
    pub fn with_shared_model(
        model: Arc<M>,
        state: SSEState,
        beta: f64,
        rng: R,
    ) -> Result<Self, SamplerError> {
        if !beta.is_finite() || beta <= 0.0 {
            return Err(SamplerError::InvalidBeta(beta));
        }
        if model.diagonal_term_indices().is_empty() {
            return Err(SamplerError::NoDiagonalTerms);
        }
        state.validate_trace(model.as_ref())?;

        Ok(Self {
            model,
            state,
            beta,
            rng,
        })
    }

    /// Borrows the immutable model.
    #[must_use]
    pub fn model(&self) -> &M {
        self.model.as_ref()
    }

    /// Borrows the current Monte Carlo state.
    #[must_use]
    pub fn state(&self) -> &SSEState {
        &self.state
    }

    /// Returns the inverse temperature used in update probabilities.
    #[must_use]
    pub fn beta(&self) -> f64 {
        self.beta
    }

    /// Instantaneous SSE energy estimator, including the decomposition shift.
    #[must_use]
    pub fn energy_estimator(&self) -> f64 {
        self.model.energy_shift() - self.state.expansion_order() as f64 / self.beta
    }

    /// Grow the fixed-length string when less than 25% headroom remains.
    ///
    /// Returns `true` if the cutoff grew. Growth preserves all existing
    /// operators and appends identity positions.
    pub fn ensure_operator_headroom(&mut self) -> bool {
        let cutoff = self.state.operator_string_length();
        let expansion_order = self.state.expansion_order();
        let minimum_empty = (cutoff / 4).max(16);
        if cutoff - expansion_order >= minimum_empty {
            return false;
        }

        let growth = (cutoff / 3).max(16);
        self.state.grow_operator_string(cutoff + growth);
        true
    }

    /// Perform a Metropolis insertion/removal sweep through the operator string.
    ///
    /// # Errors
    ///
    /// Returns model or trace-closure errors if the current state is invalid.
    pub fn diagonal_sweep(&mut self) -> Result<DiagonalSweepStats, SamplerError> {
        let mut stats = DiagonalSweepStats::default();
        let mut propagated = self.state.basis_state.clone();
        let mut expansion_order = self.state.expansion_order();
        let cutoff = self.state.operator_string.len();
        let diagonal_terms = self.model.diagonal_term_indices();
        let num_diagonal_terms = diagonal_terms.len() as f64;

        for (position, operator) in self.state.operator_string.iter_mut().enumerate() {
            match operator.kind {
                OperatorKind::Identity => {
                    stats.insertions_proposed += 1;
                    let selected = diagonal_terms[self.rng.random_range(0..diagonal_terms.len())];
                    let weight = self.model.matrix_element(selected as usize, &propagated)?;
                    if weight == 0.0 {
                        continue;
                    }

                    let empty_slots = cutoff - expansion_order;
                    let acceptance = self.beta * num_diagonal_terms * weight / empty_slots as f64;
                    if self.rng.random::<f64>() < acceptance.min(1.0) {
                        *operator = Operator::diagonal(selected);
                        expansion_order += 1;
                        stats.insertions_accepted += 1;
                    }
                }
                OperatorKind::Diagonal => {
                    stats.removals_proposed += 1;
                    let term_index =
                        operator
                            .term_index()
                            .ok_or(SseModelError::InvalidOperatorReference {
                                position,
                                term_index: operator.term_index,
                            })?;
                    let weight = self.model.matrix_element(term_index, &propagated)?;
                    if weight <= 0.0 {
                        return Err(SseModelError::ZeroMatrixElement { term_index }.into());
                    }

                    let acceptance = (cutoff - expansion_order + 1) as f64
                        / (self.beta * num_diagonal_terms * weight);
                    if self.rng.random::<f64>() < acceptance.min(1.0) {
                        *operator = Operator::identity();
                        expansion_order -= 1;
                        stats.removals_accepted += 1;
                    }
                }
                OperatorKind::OffDiagonal => {
                    let term_index =
                        operator
                            .term_index()
                            .ok_or(SseModelError::InvalidOperatorReference {
                                position,
                                term_index: operator.term_index,
                            })?;
                    self.model.apply_off_diagonal(term_index, &mut propagated)?;
                }
            }
        }

        if propagated != self.state.basis_state {
            return Err(SseModelError::TraceNotClosed.into());
        }

        debug_assert_eq!(expansion_order, self.state.expansion_order());
        self.ensure_operator_headroom();
        Ok(stats)
    }

    /// Perform the standard transverse-field Ising linked-cluster update.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerError::UnsupportedClusterUpdate`] if the model does not
    /// advertise the required TFIM vertex breakup, plus model propagation errors.
    pub fn tfim_cluster_sweep(&mut self) -> Result<ClusterSweepStats, SamplerError> {
        if !self.model.supports_tfim_cluster_update() {
            return Err(SamplerError::UnsupportedClusterUpdate);
        }

        self.cluster_sweep(false)
    }

    /// Perform a trace-preserving cluster proposal followed by a Metropolis
    /// correction for occupation-dependent Rydberg diagonal weights.
    ///
    /// This global proposal is retained primarily as a correctness reference;
    /// its acceptance may be poor for large or strongly interacting systems.
    pub fn rydberg_global_cluster_sweep(&mut self) -> Result<ClusterSweepStats, SamplerError> {
        self.cluster_sweep(true)
    }

    /// Perform local world-line Metropolis moves for occupation-dependent
    /// diagonal weights. Pair moves toggle two transverse vertices on one
    /// site; whole-line moves change that site's time-boundary spin.
    ///
    /// # Errors
    ///
    /// Returns model evaluation or trace-closure errors.
    pub fn rydberg_local_sweep(&mut self) -> Result<ClusterSweepStats, SamplerError> {
        let mut stats = ClusterSweepStats::default();
        for site in 0..self.model.num_sites() {
            let transverse_positions: Vec<_> = self
                .state
                .operator_string
                .iter()
                .enumerate()
                .filter_map(|(position, operator)| {
                    let term_index = operator.term_index()?;
                    let term = self.model.term(term_index)?;
                    let is_site = term.sites() == SseTermSites::Site(site as u32);
                    (is_site && self.model.transverse_partner(term_index).is_some())
                        .then_some(position)
                })
                .collect();

            stats.proposals += 1;
            let original_log_weight = self.state.propagate(self.model.as_ref())?.log_weight;
            let original_basis_state = self.state.basis_state.clone();
            let original_operator_string = self.state.operator_string.clone();
            let pair_move = transverse_positions.len() >= 2 && self.rng.random::<bool>();
            if pair_move {
                let left_index = self.rng.random_range(0..transverse_positions.len());
                let mut right_index = self.rng.random_range(0..transverse_positions.len() - 1);
                if right_index >= left_index {
                    right_index += 1;
                }
                for index in [left_index, right_index] {
                    let position = transverse_positions[index];
                    let term_index = self.state.operator_string[position]
                        .term_index()
                        .expect("transverse positions contain non-identity operators");
                    let partner = self
                        .model
                        .transverse_partner(term_index)
                        .expect("transverse positions contain partnered operators");
                    self.state.operator_string[position] = Operator {
                        kind: self.model.operator_kind(partner as usize)?,
                        term_index: partner,
                    };
                }
                stats.vertices_toggled += 2;
            } else {
                self.state.basis_state[site] = self.state.basis_state[site].flip();
                stats.flipped_clusters += 1;
            }

            let proposed = match self.state.propagate(self.model.as_ref()) {
                Ok(proposed) if proposed.trace_closed => proposed,
                Ok(_) => return Err(SseModelError::TraceNotClosed.into()),
                Err(SseModelError::ZeroMatrixElement { .. })
                | Err(SseModelError::NegativeMatrixElement { .. }) => {
                    self.state.basis_state = original_basis_state;
                    self.state.operator_string = original_operator_string;
                    if pair_move {
                        stats.vertices_toggled -= 2;
                    } else {
                        stats.flipped_clusters -= 1;
                    }
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let log_acceptance = proposed.log_weight - original_log_weight;
            if self.rng.random::<f64>() < metropolis_acceptance(log_acceptance) {
                stats.proposals_accepted += 1;
            } else {
                self.state.basis_state = original_basis_state;
                self.state.operator_string = original_operator_string;
                if pair_move {
                    stats.vertices_toggled -= 2;
                } else {
                    stats.flipped_clusters -= 1;
                }
            }
        }
        Ok(stats)
    }

    fn cluster_sweep(
        &mut self,
        metropolis_correction: bool,
    ) -> Result<ClusterSweepStats, SamplerError> {
        let original_basis_state = self.state.basis_state.clone();
        let original_operator_string = self.state.operator_string.clone();
        let original_log_weight = if metropolis_correction {
            self.state.propagate(self.model.as_ref())?.log_weight
        } else {
            0.0
        };

        let num_sites = self.model.num_sites();
        let mut first_leg = vec![None; num_sites];
        let mut last_leg = vec![None; num_sites];
        let mut union_find = UnionFind::default();
        let mut vertices = Vec::new();

        for (position, operator) in self.state.operator_string.iter().enumerate() {
            let Some(term_index) = operator.term_index() else {
                continue;
            };
            let term =
                *self
                    .model
                    .term(term_index)
                    .ok_or(SseModelError::InvalidOperatorReference {
                        position,
                        term_index: operator.term_index,
                    })?;

            match term.sites() {
                SseTermSites::Site(site) => {
                    let incoming = union_find.add();
                    let outgoing = union_find.add();
                    link_world_line(
                        site as usize,
                        incoming,
                        outgoing,
                        &mut first_leg,
                        &mut last_leg,
                        &mut union_find,
                    );
                    if self.model.transverse_partner(term_index).is_some() {
                        vertices.push(ClusterVertex::Site {
                            position,
                            term_index,
                            incoming,
                            outgoing,
                        });
                    } else {
                        union_find.union(incoming, outgoing);
                        vertices.push(ClusterVertex::Bond);
                    }
                }
                SseTermSites::Bond(site_i, site_j) => {
                    let i_in = union_find.add();
                    let i_out = union_find.add();
                    let j_in = union_find.add();
                    let j_out = union_find.add();
                    link_world_line(
                        site_i as usize,
                        i_in,
                        i_out,
                        &mut first_leg,
                        &mut last_leg,
                        &mut union_find,
                    );
                    link_world_line(
                        site_j as usize,
                        j_in,
                        j_out,
                        &mut first_leg,
                        &mut last_leg,
                        &mut union_find,
                    );
                    union_find.union(i_in, i_out);
                    union_find.union(i_in, j_in);
                    union_find.union(i_in, j_out);
                    vertices.push(ClusterVertex::Bond);
                }
            }
        }

        for site in 0..num_sites {
            if let (Some(first), Some(last)) = (first_leg[site], last_leg[site]) {
                union_find.union(first, last);
            }
        }

        let mut flip = vec![false; union_find.len()];
        let mut assigned = vec![false; union_find.len()];
        let mut stats = ClusterSweepStats::default();
        for leg in 0..union_find.len() {
            let root = union_find.find(leg);
            if !assigned[root] {
                assigned[root] = true;
                flip[root] = self.rng.random::<bool>();
                stats.clusters += 1;
                stats.flipped_clusters += usize::from(flip[root]);
            }
        }

        for (site, &first) in first_leg.iter().enumerate() {
            match first {
                Some(leg) if flip[union_find.find(leg)] => {
                    self.state.basis_state[site] = self.state.basis_state[site].flip();
                }
                None => {
                    stats.clusters += 1;
                    if self.rng.random::<bool>() {
                        self.state.basis_state[site] = self.state.basis_state[site].flip();
                        stats.flipped_clusters += 1;
                    }
                }
                _ => {}
            }
        }

        for vertex in vertices {
            let ClusterVertex::Site {
                position,
                term_index,
                incoming,
                outgoing,
            } = vertex
            else {
                continue;
            };
            let toggled = flip[union_find.find(incoming)] != flip[union_find.find(outgoing)];
            if toggled {
                let partner = self
                    .model
                    .transverse_partner(term_index)
                    .ok_or(SamplerError::UnsupportedClusterUpdate)?;
                let kind = self.model.operator_kind(partner as usize)?;
                self.state.operator_string[position] = Operator {
                    kind,
                    term_index: partner,
                };
                stats.vertices_toggled += 1;
            }
        }

        stats.proposals = 1;
        if metropolis_correction {
            let proposed = match self.state.propagate(self.model.as_ref()) {
                Ok(proposed) => proposed,
                Err(SseModelError::ZeroMatrixElement { .. })
                | Err(SseModelError::NegativeMatrixElement { .. }) => {
                    self.state.basis_state = original_basis_state;
                    self.state.operator_string = original_operator_string;
                    stats.flipped_clusters = 0;
                    stats.vertices_toggled = 0;
                    return Ok(stats);
                }
                Err(error) => return Err(error.into()),
            };
            if !proposed.trace_closed {
                return Err(SseModelError::TraceNotClosed.into());
            }
            let proposed_log_weight = proposed.log_weight;
            let log_acceptance = proposed_log_weight - original_log_weight;
            let accepted = self.rng.random::<f64>() < metropolis_acceptance(log_acceptance);
            if accepted {
                stats.proposals_accepted = 1;
            } else {
                self.state.basis_state = original_basis_state;
                self.state.operator_string = original_operator_string;
                stats.flipped_clusters = 0;
                stats.vertices_toggled = 0;
            }
        } else {
            self.state.validate_trace(self.model.as_ref())?;
            stats.proposals_accepted = 1;
        }
        Ok(stats)
    }

    /// Performs one TFIM diagonal sweep followed by one linked-cluster sweep.
    pub fn tfim_sweep(&mut self) -> Result<(DiagonalSweepStats, ClusterSweepStats), SamplerError> {
        let diagonal = self.diagonal_sweep()?;
        let clusters = self.tfim_cluster_sweep()?;
        Ok((diagonal, clusters))
    }

    /// Performs one Rydberg diagonal sweep followed by local world-line moves.
    pub fn rydberg_sweep(
        &mut self,
    ) -> Result<(DiagonalSweepStats, ClusterSweepStats), SamplerError> {
        let diagonal = self.diagonal_sweep()?;
        let clusters = self.rydberg_local_sweep()?;
        Ok((diagonal, clusters))
    }

    /// Performs one Rydberg diagonal sweep followed by the global reference update.
    pub fn rydberg_global_sweep(
        &mut self,
    ) -> Result<(DiagonalSweepStats, ClusterSweepStats), SamplerError> {
        let diagonal = self.diagonal_sweep()?;
        let clusters = self.rydberg_global_cluster_sweep()?;
        Ok((diagonal, clusters))
    }

    /// Runs TFIM thermalization and measurement phases.
    ///
    /// One expansion-order measurement is recorded after each group of
    /// `sweeps_per_measurement` updates.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerError::InvalidSimulationConfig`] when either
    /// measurement count is zero, or forwards update/model failures.
    pub fn run_tfim(
        &mut self,
        config: SimulationConfig,
    ) -> Result<SimulationResults, SamplerError> {
        self.run_tfim_inner(config, false)
            .map(|recorded| recorded.simulation)
    }

    /// Runs TFIM sampling and retains the complete expansion-order series.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`SseSampler::run_tfim`].
    pub fn run_tfim_recorded(
        &mut self,
        config: SimulationConfig,
    ) -> Result<RecordedSimulationResults, SamplerError> {
        self.run_tfim_inner(config, true)
    }

    fn run_tfim_inner(
        &mut self,
        config: SimulationConfig,
        retain_measurements: bool,
    ) -> Result<RecordedSimulationResults, SamplerError> {
        if config.measurement_sweeps == 0 {
            return Err(SamplerError::InvalidSimulationConfig(
                "measurement_sweeps must be greater than zero",
            ));
        }
        if config.sweeps_per_measurement == 0 {
            return Err(SamplerError::InvalidSimulationConfig(
                "sweeps_per_measurement must be greater than zero",
            ));
        }

        let run_started = Instant::now();
        let mut timing = RunTiming::default();
        let thermalization_started = Instant::now();
        for _ in 0..config.thermalization_sweeps {
            let started = Instant::now();
            self.diagonal_sweep()?;
            timing.diagonal_updates += started.elapsed();
            let started = Instant::now();
            self.tfim_cluster_sweep()?;
            timing.cluster_updates += started.elapsed();
            timing.thermalization_sweeps += 1;
        }
        timing.thermalization = thermalization_started.elapsed();

        let mut accumulator = ThermodynamicAccumulator::default();
        let mut measurements =
            retain_measurements.then(|| Vec::with_capacity(config.measurement_sweeps));
        let mut diagonal_total = DiagonalSweepStats::default();
        let mut cluster_total = ClusterSweepStats::default();
        let measurement_started = Instant::now();
        for _ in 0..config.measurement_sweeps {
            for _ in 0..config.sweeps_per_measurement {
                let started = Instant::now();
                let diagonal = self.diagonal_sweep()?;
                timing.diagonal_updates += started.elapsed();
                let started = Instant::now();
                let clusters = self.tfim_cluster_sweep()?;
                timing.cluster_updates += started.elapsed();
                diagonal_total.add_assign(diagonal);
                cluster_total.add_assign(clusters);
                timing.measurement_sweeps += 1;
            }
            let started = Instant::now();
            accumulator.record(self.state.expansion_order());
            if let Some(records) = &mut measurements {
                records.push(MeasurementRecord {
                    measurement_index: records.len(),
                    expansion_order: self.state.expansion_order(),
                });
            }
            timing.accumulation += started.elapsed();
            timing.measurements += 1;
        }
        timing.measurement = measurement_started.elapsed();

        let thermodynamics = accumulator
            .results(self.beta, self.model.energy_shift(), self.model.num_sites())
            .expect("measurement_sweeps was validated as nonzero");
        timing.total = run_started.elapsed();
        Ok(RecordedSimulationResults {
            simulation: SimulationResults {
                thermodynamics,
                diagonal: diagonal_total,
                clusters: cluster_total,
                timing,
            },
            measurements: measurements.unwrap_or_default(),
        })
    }

    /// Runs a Rydberg simulation using local world-line updates.
    ///
    /// # Errors
    ///
    /// Returns invalid-configuration, update, or model errors.
    pub fn run_rydberg(
        &mut self,
        config: SimulationConfig,
    ) -> Result<SimulationResults, SamplerError> {
        self.run_with_rydberg_update(config, false, false)
            .map(|recorded| recorded.simulation)
    }

    /// Runs local Rydberg sampling and retains the expansion-order series.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`SseSampler::run_rydberg`].
    pub fn run_rydberg_recorded(
        &mut self,
        config: SimulationConfig,
    ) -> Result<RecordedSimulationResults, SamplerError> {
        self.run_with_rydberg_update(config, false, true)
    }

    /// Runs a Rydberg simulation using the global corrected reference update.
    ///
    /// This method is intended for validation against the default local update.
    ///
    /// # Errors
    ///
    /// Returns invalid-configuration, update, or model errors.
    pub fn run_rydberg_global_reference(
        &mut self,
        config: SimulationConfig,
    ) -> Result<SimulationResults, SamplerError> {
        self.run_with_rydberg_update(config, true, false)
            .map(|recorded| recorded.simulation)
    }

    /// Runs the global Rydberg reference update and retains its sample series.
    ///
    /// # Errors
    ///
    /// Returns the same failures as
    /// [`SseSampler::run_rydberg_global_reference`].
    pub fn run_rydberg_global_reference_recorded(
        &mut self,
        config: SimulationConfig,
    ) -> Result<RecordedSimulationResults, SamplerError> {
        self.run_with_rydberg_update(config, true, true)
    }

    fn run_with_rydberg_update(
        &mut self,
        config: SimulationConfig,
        global_reference: bool,
        retain_measurements: bool,
    ) -> Result<RecordedSimulationResults, SamplerError> {
        if config.measurement_sweeps == 0 {
            return Err(SamplerError::InvalidSimulationConfig(
                "measurement_sweeps must be greater than zero",
            ));
        }
        if config.sweeps_per_measurement == 0 {
            return Err(SamplerError::InvalidSimulationConfig(
                "sweeps_per_measurement must be greater than zero",
            ));
        }

        let run_started = Instant::now();
        let mut timing = RunTiming::default();
        let thermalization_started = Instant::now();
        for _ in 0..config.thermalization_sweeps {
            let started = Instant::now();
            self.diagonal_sweep()?;
            timing.diagonal_updates += started.elapsed();
            let started = Instant::now();
            if global_reference {
                self.rydberg_global_cluster_sweep()?;
            } else {
                self.rydberg_local_sweep()?;
            }
            timing.cluster_updates += started.elapsed();
            timing.thermalization_sweeps += 1;
        }
        timing.thermalization = thermalization_started.elapsed();

        let mut accumulator = ThermodynamicAccumulator::default();
        let mut measurements =
            retain_measurements.then(|| Vec::with_capacity(config.measurement_sweeps));
        let mut diagonal_total = DiagonalSweepStats::default();
        let mut cluster_total = ClusterSweepStats::default();
        let measurement_started = Instant::now();
        for _ in 0..config.measurement_sweeps {
            for _ in 0..config.sweeps_per_measurement {
                let started = Instant::now();
                let diagonal = self.diagonal_sweep()?;
                timing.diagonal_updates += started.elapsed();
                let started = Instant::now();
                let clusters = if global_reference {
                    self.rydberg_global_cluster_sweep()?
                } else {
                    self.rydberg_local_sweep()?
                };
                timing.cluster_updates += started.elapsed();
                diagonal_total.add_assign(diagonal);
                cluster_total.add_assign(clusters);
                timing.measurement_sweeps += 1;
            }
            let started = Instant::now();
            accumulator.record(self.state.expansion_order());
            if let Some(records) = &mut measurements {
                records.push(MeasurementRecord {
                    measurement_index: records.len(),
                    expansion_order: self.state.expansion_order(),
                });
            }
            timing.accumulation += started.elapsed();
            timing.measurements += 1;
        }
        timing.measurement = measurement_started.elapsed();
        let thermodynamics = accumulator
            .results(self.beta, self.model.energy_shift(), self.model.num_sites())
            .expect("measurement_sweeps was validated as nonzero");
        timing.total = run_started.elapsed();
        Ok(RecordedSimulationResults {
            simulation: SimulationResults {
                thermodynamics,
                diagonal: diagonal_total,
                clusters: cluster_total,
                timing,
            },
            measurements: measurements.unwrap_or_default(),
        })
    }
}

impl DiagonalSweepStats {
    fn add_assign(&mut self, other: Self) {
        self.insertions_proposed += other.insertions_proposed;
        self.insertions_accepted += other.insertions_accepted;
        self.removals_proposed += other.removals_proposed;
        self.removals_accepted += other.removals_accepted;
    }
}

impl ClusterSweepStats {
    fn add_assign(&mut self, other: Self) {
        self.clusters += other.clusters;
        self.flipped_clusters += other.flipped_clusters;
        self.vertices_toggled += other.vertices_toggled;
        self.proposals += other.proposals;
        self.proposals_accepted += other.proposals_accepted;
    }
}

#[derive(Clone, Copy, Debug)]
enum ClusterVertex {
    Site {
        position: usize,
        term_index: usize,
        incoming: usize,
        outgoing: usize,
    },
    Bond,
}

#[derive(Default)]
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn add(&mut self) -> usize {
        let index = self.parent.len();
        self.parent.push(index);
        self.rank.push(0);
        index
    }

    fn len(&self) -> usize {
        self.parent.len()
    }

    fn find(&mut self, index: usize) -> usize {
        if self.parent[index] != index {
            self.parent[index] = self.find(self.parent[index]);
        }
        self.parent[index]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.rank[left_root] < self.rank[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }
        self.parent[right_root] = left_root;
        if self.rank[left_root] == self.rank[right_root] {
            self.rank[left_root] += 1;
        }
    }
}

fn link_world_line(
    site: usize,
    incoming: usize,
    outgoing: usize,
    first_leg: &mut [Option<usize>],
    last_leg: &mut [Option<usize>],
    union_find: &mut UnionFind,
) {
    if let Some(previous) = last_leg[site] {
        union_find.union(previous, incoming);
    } else {
        first_leg[site] = Some(incoming);
    }
    last_leg[site] = Some(outgoing);
}

fn metropolis_acceptance(log_weight_ratio: f64) -> f64 {
    log_weight_ratio.min(0.0).exp()
}

#[cfg(test)]
mod tests {
    use rand::{rngs::StdRng, SeedableRng};

    use super::*;
    use crate::{BoundaryCondition, Geometry, LocalSseModel, Spin};

    #[test]
    fn diagonal_sweep_preserves_a_valid_trace() {
        let geometry = Geometry::chain(2, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::tfim(&geometry, &[(0, 1)], 1.0, 0.5).unwrap();
        let state = SSEState::new(&model, vec![Spin::Up, Spin::Down], 32).unwrap();
        let rng = StdRng::seed_from_u64(7);
        let mut sampler = SseSampler::new(model, state, 2.0, rng).unwrap();

        for _ in 0..100 {
            sampler.diagonal_sweep().unwrap();
            sampler.state().validate_trace(sampler.model()).unwrap();
        }

        assert!(sampler.state().expansion_order() > 0);
        assert!(sampler.energy_estimator().is_finite());
    }

    #[test]
    fn tfim_cluster_sweep_preserves_trace_and_changes_vertex_kinds() {
        let geometry = Geometry::chain(4, BoundaryCondition::Periodic).unwrap();
        let pairs = geometry.pairs_at_distance_squared(1.0, 1.0e-12).unwrap();
        let model = LocalSseModel::tfim(&geometry, &pairs, 1.0, 0.8).unwrap();
        let state = SSEState::new(&model, vec![Spin::Up; 4], 64).unwrap();
        let rng = StdRng::seed_from_u64(19);
        let mut sampler = SseSampler::new(model, state, 3.0, rng).unwrap();
        let mut toggles = 0;

        for _ in 0..200 {
            sampler.diagonal_sweep().unwrap();
            toggles += sampler.tfim_cluster_sweep().unwrap().vertices_toggled;
            sampler.state().validate_trace(sampler.model()).unwrap();
        }

        assert!(toggles > 0);
        assert!(sampler
            .state()
            .operator_string()
            .iter()
            .any(|operator| operator.kind == OperatorKind::OffDiagonal));
    }

    #[test]
    fn rydberg_rejects_tfim_cluster_rules() {
        let geometry = Geometry::chain(2, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::rydberg(&geometry, 1.0, 0.5, 1.0).unwrap();
        let state = SSEState::new(&model, vec![Spin::Down; 2], 16).unwrap();
        let rng = StdRng::seed_from_u64(2);
        let mut sampler = SseSampler::new(model, state, 1.0, rng).unwrap();

        assert!(matches!(
            sampler.tfim_cluster_sweep(),
            Err(SamplerError::UnsupportedClusterUpdate)
        ));
    }

    #[test]
    fn rydberg_metropolis_clusters_preserve_trace_and_explore_spin_flips() {
        let geometry = Geometry::square(2, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::rydberg(&geometry, 1.0, 1.0, 1.0).unwrap();
        let state = SSEState::new(&model, vec![Spin::Down; 4], 128).unwrap();
        let rng = StdRng::seed_from_u64(24301);
        let mut sampler = SseSampler::new(model, state, 4.0, rng).unwrap();
        let mut accepted = 0;

        for _ in 0..1_000 {
            sampler.diagonal_sweep().unwrap();
            let stats = sampler.rydberg_local_sweep().unwrap();
            accepted += stats.proposals_accepted;
            sampler.state().validate_trace(sampler.model()).unwrap();
        }

        assert!(accepted > 0);
        assert!(sampler
            .state()
            .operator_string()
            .iter()
            .any(|operator| operator.kind == OperatorKind::OffDiagonal));
    }

    #[test]
    fn single_spin_tfim_matches_exact_thermal_energy() {
        let geometry = Geometry::chain(1, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::tfim(&geometry, &[], 0.0, 1.0).unwrap();
        let state = SSEState::new(&model, vec![Spin::Up], 32).unwrap();
        let rng = StdRng::seed_from_u64(1234);
        let mut sampler = SseSampler::new(model, state, 2.0, rng).unwrap();
        let result = sampler
            .run_tfim(SimulationConfig {
                thermalization_sweeps: 2_000,
                measurement_sweeps: 30_000,
                sweeps_per_measurement: 1,
            })
            .unwrap();
        let exact = -2.0_f64.tanh();

        assert!((result.thermodynamics.energy - exact).abs() < 0.03);
    }

    #[test]
    fn rejects_invalid_beta() {
        let geometry = Geometry::chain(1, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::tfim(&geometry, &[], 0.0, 1.0).unwrap();
        let state = SSEState::new(&model, vec![Spin::Up], 8).unwrap();
        let rng = StdRng::seed_from_u64(1);

        assert!(matches!(
            SseSampler::new(model, state, 0.0, rng),
            Err(SamplerError::InvalidBeta(0.0))
        ));
    }

    #[test]
    fn metropolis_transition_satisfies_local_detailed_balance() {
        for (left_weight, right_weight) in [(0.2_f64, 3.0_f64), (4.5, 0.7), (2.0, 2.0)] {
            let forward = metropolis_acceptance((right_weight / left_weight).ln());
            let reverse = metropolis_acceptance((left_weight / right_weight).ln());
            assert!((left_weight * forward - right_weight * reverse).abs() < 1.0e-12);
        }
    }
}
