//! Building blocks for stochastic-series-expansion simulations.
//!
//! The crate provides lattice geometry, physical Hamiltonian descriptions,
//! sign-safe local SSE decompositions, and operator-string propagation.
//!
//! # Typical workflow
//!
//! 1. Construct a validated [`Geometry`].
//! 2. Decompose a supported Hamiltonian into a [`LocalSseModel`].
//! 3. Create an [`SSEState`] with a basis state and operator-string cutoff.
//! 4. Build an [`SseSampler`] and run model-appropriate sweeps.
//!
//! The local decompositions include a constant [`SseModel::energy_shift`]
//! chosen to make all sampled matrix elements non-negative. Reported energy
//! estimators restore this shift automatically.
//!
//! # Example
//!
//! ```
//! use rand::{rngs::StdRng, SeedableRng};
//! use sse::{
//!     BoundaryCondition, Geometry, LocalSseModel, SSEState, SimulationConfig,
//!     Spin, SseModel, SseSampler,
//! };
//!
//! let geometry = Geometry::chain(4, BoundaryCondition::Periodic)?;
//! let pairs = geometry.pairs_at_distance_squared(1.0, 1.0e-12)?;
//! let model = LocalSseModel::tfim(&geometry, &pairs, 1.0, 0.5)?;
//! let state = SSEState::new(&model, vec![Spin::Up; model.num_sites()], 64)?;
//! let rng = StdRng::seed_from_u64(7);
//! let mut sampler = SseSampler::new(model, state, 2.0, rng)?;
//! let results = sampler.run_tfim(SimulationConfig {
//!     thermalization_sweeps: 10,
//!     measurement_sweeps: 20,
//!     sweeps_per_measurement: 1,
//! })?;
//! assert_eq!(results.thermodynamics.samples, 20);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]

mod artifacts;
mod config;
mod core;
mod geometry;
mod hamiltonian;
mod lattice;
mod runner;
mod sse;

pub use artifacts::{
    inspect_run, ArtifactError, ChainArtifact, ChainDiagnostics, ChainSummary, CheckpointIndex,
    RunInspection, RunManifest, RunStatus, RunSummary, ThermodynamicArtifact, TimingArtifact,
    UpdateStatistics, ARTIFACT_SCHEMA_VERSION,
};
pub use config::{
    BoundaryConfig, ConfigError, ExecutionSettings, GeometryConfig, InitialState, ModelConfig,
    RunConfig, RydbergUpdate, SimulationSettings, RUN_SCHEMA_VERSION,
};
pub use core::{OperatorKind, Spin};
pub use geometry::{BoundaryCondition, Geometry, GeometryError};
pub use hamiltonian::{
    Hamiltonian, HamiltonianError, HamiltonianTerm, LocalOperatorKind, TermSupport,
};
pub use lattice::{Bond, Lattice, LatticeError, PairSelection};
pub use runner::{run_to_directory, RunMode, RunOutcome, RunnerError};
pub use sse::{
    derive_chain_seed, run_parallel_tfim, ChainResults, ClusterSweepStats, CombinedEnergyResults,
    DiagonalSweepStats, LocalSseModel, MeasurementRecord, Operator, ParallelSimulationConfig,
    ParallelSimulationError, ParallelSimulationResults, PropagationResult,
    RecordedSimulationResults, RunTiming, SSEState, SamplerError, SimulationConfig,
    SimulationResults, SseModel, SseModelError, SseSampler, SseTerm, SseTermSites,
    ThermodynamicAccumulator, ThermodynamicResults,
};
