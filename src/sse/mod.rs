//! Sign-safe local decompositions and stochastic-series-expansion sampling.
//!
//! A model represents the physical Hamiltonian as
//! `H = energy_shift - sum_a B_a`, where every sampled local matrix element of
//! `B_a` is non-negative. [`SSEState`] stores a padded operator string over the
//! `B_a`, and [`SseSampler`] updates that string while preserving the trace.

use std::error::Error;
use std::fmt;

use crate::core::{OperatorKind, Spin};

mod local_model;
mod measurements;
mod parallel;
mod sampler;
mod state;

pub use local_model::LocalSseModel;
pub use measurements::{ThermodynamicAccumulator, ThermodynamicResults};
pub use parallel::{
    derive_chain_seed, run_parallel_tfim, ChainResults, CombinedEnergyResults,
    ParallelSimulationConfig, ParallelSimulationError, ParallelSimulationResults,
};
pub use sampler::{
    ClusterSweepStats, DiagonalSweepStats, MeasurementRecord, RecordedSimulationResults, RunTiming,
    SamplerError, SimulationConfig, SimulationResults, SseSampler,
};
pub use state::{Operator, PropagationResult, SSEState};

/// A non-negative local operator used by the SSE expansion.
///
/// Coefficients and shifts are stored explicitly so [`SseModel::matrix_element`]
/// can evaluate a term from the current Pauli-z basis state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SseTerm {
    /// Diagonal single-site constant used as the partner of a spin-flip
    /// vertex in cluster and loop updates:
    ///
    /// B_i = a I
    SiteConstant {
        /// Site on which the identity partner is placed.
        site: u32,
        /// Non-negative constant matrix element.
        amplitude: f64,
    },

    /// Diagonal two-site TFIM operator:
    ///
    /// B_ij = J (C + s_i s_j)
    TfimBond {
        /// First bond endpoint.
        site_i: u32,
        /// Second bond endpoint.
        site_j: u32,
        /// Non-negative Ising coupling.
        coupling: f64,
        /// Additive factor ensuring non-negative diagonal weight.
        shift: f64,
    },

    /// Off-diagonal single-site spin flip:
    ///
    /// B_i = h σᵢˣ
    SpinFlip {
        /// Site whose spin is flipped.
        site: u32,
        /// Non-negative off-diagonal matrix element.
        amplitude: f64,
    },

    /// Diagonal onsite Rydberg term:
    ///
    /// B_i = C + δ n_i
    RydbergDetuning {
        /// Site whose occupation is evaluated.
        site: u32,
        /// Signed detuning contribution in the SSE operator convention.
        detuning: f64,
        /// Additive constant ensuring non-negative matrix elements.
        shift: f64,
    },

    /// Diagonal two-site Rydberg interaction:
    ///
    /// B_ij = C - V_ij n_i n_j
    RydbergInteraction {
        /// First interacting site.
        site_i: u32,
        /// Second interacting site.
        site_j: u32,
        /// Signed `C6 / r^6` interaction.
        interaction: f64,
        /// Additive constant ensuring non-negative matrix elements.
        shift: f64,
    },
}

impl SseTerm {
    /// Returns whether this term is diagonal or off-diagonal in the spin basis.
    #[must_use]
    pub fn operator_kind(self) -> OperatorKind {
        match self {
            Self::SpinFlip { .. } => OperatorKind::OffDiagonal,
            Self::SiteConstant { .. }
            | Self::TfimBond { .. }
            | Self::RydbergDetuning { .. }
            | Self::RydbergInteraction { .. } => OperatorKind::Diagonal,
        }
    }

    /// Returns the one- or two-site support of this term.
    #[must_use]
    pub fn sites(self) -> SseTermSites {
        match self {
            Self::SiteConstant { site, .. }
            | Self::SpinFlip { site, .. }
            | Self::RydbergDetuning { site, .. } => SseTermSites::Site(site),
            Self::TfimBond { site_i, site_j, .. }
            | Self::RydbergInteraction { site_i, site_j, .. } => SseTermSites::Bond(site_i, site_j),
        }
    }
}

/// Site support of an [`SseTerm`], without its coefficients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SseTermSites {
    /// A single site.
    Site(u32),
    /// A two-site bond.
    Bond(u32, u32),
}

/// Failure while constructing, evaluating, or propagating an SSE model.
#[derive(Debug)]
pub enum SseModelError {
    /// The model contains no local terms.
    EmptyModel,

    /// A basis state does not contain one spin per model site.
    InvalidBasisStateLength {
        /// Received number of spins.
        received: usize,
        /// Required number of spins.
        expected: usize,
    },

    /// The padded operator-string cutoff is zero.
    InvalidOperatorStringLength,

    /// A local term refers to a site outside the model.
    InvalidSite {
        /// Invalid site ID.
        site: u32,
        /// Number of model sites.
        num_sites: usize,
    },

    /// A term index lies outside the model's local-term array.
    InvalidTermIndex {
        /// Invalid zero-based term index.
        term_index: usize,
        /// Number of model terms.
        num_terms: usize,
    },

    /// An operator-string entry has the wrong diagonal classification.
    InvalidOperatorKind {
        /// Referenced term index.
        term_index: usize,
        /// Classification required by the model term.
        expected: OperatorKind,
        /// Classification stored in the operator string.
        received: OperatorKind,
    },

    /// A model coefficient violated its documented domain.
    InvalidCoefficient {
        /// Human-readable coefficient or invariant name.
        name: &'static str,
        /// Invalid numeric value.
        value: f64,
    },

    /// A supposedly sign-safe term produced a negative matrix element.
    NegativeMatrixElement {
        /// Term that produced the value.
        term_index: usize,
        /// Negative matrix element.
        value: f64,
    },

    /// An operator acts with zero weight on the propagated state.
    ZeroMatrixElement {
        /// Term with zero matrix element.
        term_index: usize,
    },

    /// An operator-string entry refers to a term that does not exist.
    InvalidOperatorReference {
        /// Position in the padded operator string.
        position: usize,
        /// Invalid referenced term index.
        term_index: u32,
    },

    /// Propagation around imaginary time did not return to the initial state.
    TraceNotClosed,

    /// Geometry construction or distance evaluation failed.
    GeometryError(String),
}

impl fmt::Display for SseModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyModel => {
                write!(f, "SSE model must contain at least one local term")
            }

            Self::InvalidBasisStateLength { received, expected } => {
                write!(
                    f,
                    "basis-state length {received} does not match model site count {expected}"
                )
            }

            Self::InvalidOperatorStringLength => {
                write!(f, "operator string length M must be greater than zero")
            }

            Self::InvalidSite { site, num_sites } => {
                write!(
                    f,
                    "site {site} is invalid for a model with {num_sites} sites"
                )
            }

            Self::InvalidTermIndex {
                term_index,
                num_terms,
            } => {
                write!(
                    f,
                    "term index {term_index} is invalid for a model with \
                     {num_terms} terms"
                )
            }

            Self::InvalidOperatorKind {
                term_index,
                expected,
                received,
            } => {
                write!(
                    f,
                    "term {term_index} requires operator kind {expected:?}, \
                     but received {received:?}"
                )
            }

            Self::InvalidCoefficient { name, value } => {
                write!(f, "{name} must be finite and non-negative; got {value}")
            }

            Self::NegativeMatrixElement { term_index, value } => {
                write!(
                    f,
                    "term {term_index} produced negative matrix element {value}"
                )
            }

            Self::ZeroMatrixElement { term_index } => {
                write!(
                    f,
                    "term {term_index} has zero matrix element on the current state"
                )
            }

            Self::InvalidOperatorReference {
                position,
                term_index,
            } => {
                write!(
                    f,
                    "operator-string position {position} refers to invalid \
                     term index {term_index}"
                )
            }

            Self::TraceNotClosed => {
                write!(
                    f,
                    "propagated basis state does not return to its initial state"
                )
            }

            Self::GeometryError(message) => {
                write!(f, "geometry error: {message}")
            }
        }
    }
}

impl Error for SseModelError {}

/// Interface required by the generic [`SseSampler`].
///
/// Implementations must use the convention
/// `H = energy_shift() - sum(term)` and must return non-negative local matrix
/// elements for every state accepted by the sampler.
pub trait SseModel {
    /// Returns the number of spins in a basis state.
    fn num_sites(&self) -> usize;

    /// Returns the number of addressable local SSE terms.
    fn num_terms(&self) -> usize;

    /// Returns the constant needed to recover physical energies.
    fn energy_shift(&self) -> f64;

    /// Borrows a local term by zero-based index.
    fn term(&self, term_index: usize) -> Option<&SseTerm>;

    /// Term indices eligible for diagonal insertion and removal updates.
    fn diagonal_term_indices(&self) -> &[u32];

    /// Whether every vertex in this model supports the transverse-field Ising
    /// cluster breakup implemented by the sampler.
    fn supports_tfim_cluster_update(&self) -> bool;

    /// Matching diagonal/off-diagonal single-site vertex, when one exists.
    fn transverse_partner(&self, term_index: usize) -> Option<u32>;

    /// Returns the required operator-string classification for a term.
    ///
    /// # Errors
    ///
    /// Returns [`SseModelError::InvalidTermIndex`] for an out-of-range index.
    fn operator_kind(&self, term_index: usize) -> Result<OperatorKind, SseModelError>;

    /// Evaluates a local term on the current Pauli-z basis state.
    ///
    /// Implementations must reject materially negative weights. Tiny negative
    /// floating-point roundoff may be clamped to zero.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid state length, term index, or matrix element.
    fn matrix_element(&self, term_index: usize, basis_state: &[Spin])
        -> Result<f64, SseModelError>;

    /// Applies an off-diagonal local term to a mutable basis state.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid indices or diagonal terms.
    fn apply_off_diagonal(
        &self,
        term_index: usize,
        basis_state: &mut [Spin],
    ) -> Result<(), SseModelError>;
}
