//! Physical Hamiltonian terms independent of their SSE decomposition.
//!
//! These types describe operators and coefficients directly. Sampling uses
//! [`crate::LocalSseModel`], whose sign-safe local terms may include additional
//! diagonal shifts.

use std::error::Error;
use std::fmt;

use crate::geometry::Geometry;
use crate::lattice::{Lattice, PairSelection};

/// Local operator represented by a [`HamiltonianTerm`].
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalOperatorKind {
    /// Identity contribution.
    Identity = 0,

    /// Two-site Pauli operator `sigma_z(i) sigma_z(j)`.
    SpinZZ = 1,

    /// Single-site transverse Pauli operator `sigma_x(i)`.
    SpinX = 2,

    /// Single-site Rydberg occupation `n(i)`.
    Number = 3,

    /// Two-site Rydberg occupation product `n(i) n(j)`.
    NumberNumber = 4,
}

/// Sites on which a local Hamiltonian term acts.
///
/// Pair support is stored in canonical ascending site order when created with
/// [`TermSupport::pair`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TermSupport {
    /// A single-site term.
    Site(u32),
    /// A two-site term with endpoints `(site_i, site_j)`.
    Pair(u32, u32),
}

impl TermSupport {
    /// Creates single-site support.
    #[must_use]
    pub fn site(site: u32) -> Self {
        Self::Site(site)
    }

    /// Creates two-site support with canonical endpoint ordering.
    ///
    /// # Errors
    ///
    /// Returns a message if both endpoints identify the same site.
    pub fn pair(site_i: u32, site_j: u32) -> Result<Self, String> {
        if site_i == site_j {
            return Err(format!("a two-site term cannot act twice on site {site_i}"));
        }

        let pair = if site_i < site_j {
            (site_i, site_j)
        } else {
            (site_j, site_i)
        };

        Ok(Self::Pair(pair.0, pair.1))
    }
}

/// Failure while validating or constructing a [`Hamiltonian`].
#[derive(Debug)]
pub enum HamiltonianError {
    /// A Hamiltonian was constructed without local terms.
    EmptyHamiltonian,

    /// A term refers to a site outside the system.
    InvalidSite {
        /// Invalid site ID.
        site: u32,
        /// Number of sites in the system.
        num_sites: usize,
    },

    /// A coefficient or constant shift was NaN or infinite.
    NonFiniteCoefficient {
        /// Invalid coefficient value.
        coefficient: f64,
    },

    /// An operator was paired with the wrong number of support sites.
    IncompatibleSupport {
        /// Received support.
        support: TermSupport,
        /// Received operator.
        operator: LocalOperatorKind,
    },

    /// A Rydberg pair had a non-positive or non-finite distance.
    InvalidRydbergDistance {
        /// First site in the pair.
        site_i: u32,
        /// Second site in the pair.
        site_j: u32,
        /// Invalid distance.
        distance: f64,
    },

    /// Geometry construction or distance evaluation failed.
    GeometryError(String),

    /// Lattice bond generation failed.
    LatticeError(String),

    /// The number of terms cannot be addressed using `u32` indices.
    TooManyTerms {
        /// Requested number of terms.
        num_terms: usize,
    },
}

impl fmt::Display for HamiltonianError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyHamiltonian => {
                write!(f, "Hamiltonian must contain at least one term")
            }

            Self::InvalidSite { site, num_sites } => {
                write!(
                    f,
                    "site {site} is invalid for a system with {num_sites} sites"
                )
            }

            Self::NonFiniteCoefficient { coefficient } => {
                write!(
                    f,
                    "Hamiltonian coefficient must be finite; got {coefficient}"
                )
            }

            Self::IncompatibleSupport { support, operator } => {
                write!(
                    f,
                    "operator {operator:?} is incompatible with support {support:?}"
                )
            }

            Self::InvalidRydbergDistance {
                site_i,
                site_j,
                distance,
            } => {
                write!(
                    f,
                    "invalid distance {distance} between Rydberg sites \
                     {site_i} and {site_j}"
                )
            }

            Self::GeometryError(message) => {
                write!(f, "geometry error: {message}")
            }

            Self::LatticeError(message) => {
                write!(f, "lattice error: {message}")
            }

            Self::TooManyTerms { num_terms } => {
                write!(
                    f,
                    "Hamiltonian has {num_terms} terms, but term indices \
                     are stored as u32"
                )
            }
        }
    }
}

impl Error for HamiltonianError {}

/// One coefficient-weighted local term in a physical Hamiltonian.
///
/// This type does not include the non-negative shifts introduced by an SSE
/// decomposition. Use [`crate::SseTerm`] to inspect those sampling operators.
#[derive(Clone, Copy, Debug)]
pub struct HamiltonianTerm {
    support: TermSupport,
    operator: LocalOperatorKind,
    coefficient: f64,
}

impl HamiltonianTerm {
    /// Constructs a local term after validating coefficient and support.
    ///
    /// # Errors
    ///
    /// Returns [`HamiltonianError::NonFiniteCoefficient`] for NaN or infinity,
    /// or [`HamiltonianError::IncompatibleSupport`] when the operator arity
    /// does not match `support`.
    pub fn new(
        support: TermSupport,
        operator: LocalOperatorKind,
        coefficient: f64,
    ) -> Result<Self, HamiltonianError> {
        if !coefficient.is_finite() {
            return Err(HamiltonianError::NonFiniteCoefficient { coefficient });
        }

        Self::validate_compatibility(support, operator)?;

        Ok(Self {
            support,
            operator,
            coefficient,
        })
    }

    fn validate_compatibility(
        support: TermSupport,
        operator: LocalOperatorKind,
    ) -> Result<(), HamiltonianError> {
        let valid = matches!(
            (support, operator),
            (
                TermSupport::Site(_),
                LocalOperatorKind::SpinX | LocalOperatorKind::Number | LocalOperatorKind::Identity
            ) | (
                TermSupport::Pair(_, _),
                LocalOperatorKind::SpinZZ
                    | LocalOperatorKind::NumberNumber
                    | LocalOperatorKind::Identity
            )
        );

        if !valid {
            return Err(HamiltonianError::IncompatibleSupport { support, operator });
        }

        Ok(())
    }

    /// Returns the sites on which this term acts.
    #[must_use]
    pub fn support(&self) -> TermSupport {
        self.support
    }

    /// Returns the local operator kind.
    #[must_use]
    pub fn operator(&self) -> LocalOperatorKind {
        self.operator
    }

    /// Returns the multiplicative physical coefficient.
    #[must_use]
    pub fn coefficient(&self) -> f64 {
        self.coefficient
    }

    /// Reports whether the term is diagonal in the Pauli-z basis.
    #[must_use]
    pub fn is_diagonal_in_z_basis(&self) -> bool {
        matches!(
            self.operator,
            LocalOperatorKind::Identity
                | LocalOperatorKind::SpinZZ
                | LocalOperatorKind::Number
                | LocalOperatorKind::NumberNumber
        )
    }

    /// Reports whether the term changes a Pauli-z basis state.
    #[must_use]
    pub fn is_off_diagonal_in_z_basis(&self) -> bool {
        matches!(self.operator, LocalOperatorKind::SpinX)
    }
}

/// A validated sum of local physical Hamiltonian terms.
///
/// `constant_shift` represents a scalar multiple of the identity added to the
/// sum of [`HamiltonianTerm`] values.
#[derive(Debug)]
pub struct Hamiltonian {
    num_sites: usize,
    terms: Vec<HamiltonianTerm>,
    constant_shift: f64,
}

impl Hamiltonian {
    /// Constructs a Hamiltonian with zero constant shift.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty term list, invalid support, or too many
    /// terms for `u32` indexing.
    pub fn new(num_sites: usize, terms: Vec<HamiltonianTerm>) -> Result<Self, HamiltonianError> {
        Self::with_constant_shift(num_sites, terms, 0.0)
    }

    /// Constructs a Hamiltonian with an explicit scalar energy shift.
    ///
    /// # Errors
    ///
    /// In addition to [`Hamiltonian::new`] failures, returns an error when
    /// `constant_shift` is non-finite.
    pub fn with_constant_shift(
        num_sites: usize,
        terms: Vec<HamiltonianTerm>,
        constant_shift: f64,
    ) -> Result<Self, HamiltonianError> {
        if terms.is_empty() {
            return Err(HamiltonianError::EmptyHamiltonian);
        }

        if !constant_shift.is_finite() {
            return Err(HamiltonianError::NonFiniteCoefficient {
                coefficient: constant_shift,
            });
        }

        if terms.len() > u32::MAX as usize {
            return Err(HamiltonianError::TooManyTerms {
                num_terms: terms.len(),
            });
        }

        for term in &terms {
            Self::validate_support(term.support(), num_sites)?;
        }

        Ok(Self {
            num_sites,
            terms,
            constant_shift,
        })
    }

    fn validate_support(support: TermSupport, num_sites: usize) -> Result<(), HamiltonianError> {
        match support {
            TermSupport::Site(site) => Self::validate_site(site, num_sites),

            TermSupport::Pair(site_i, site_j) => {
                Self::validate_site(site_i, num_sites)?;
                Self::validate_site(site_j, num_sites)?;

                if site_i == site_j {
                    return Err(HamiltonianError::IncompatibleSupport {
                        support,
                        operator: LocalOperatorKind::Identity,
                    });
                }

                Ok(())
            }
        }
    }

    fn validate_site(site: u32, num_sites: usize) -> Result<(), HamiltonianError> {
        if site as usize >= num_sites {
            return Err(HamiltonianError::InvalidSite { site, num_sites });
        }

        Ok(())
    }

    /// Returns the number of sites in the Hilbert space.
    #[must_use]
    pub fn num_sites(&self) -> usize {
        self.num_sites
    }

    /// Returns the number of local terms.
    #[must_use]
    pub fn num_terms(&self) -> usize {
        self.terms.len()
    }

    /// Borrows all local terms in construction order.
    #[must_use]
    pub fn terms(&self) -> &[HamiltonianTerm] {
        &self.terms
    }

    /// Borrows a term by zero-based index.
    #[must_use]
    pub fn term(&self, term_index: usize) -> Option<&HamiltonianTerm> {
        self.terms.get(term_index)
    }

    /// Returns the scalar identity contribution to the Hamiltonian.
    #[must_use]
    pub fn constant_shift(&self) -> f64 {
        self.constant_shift
    }
}

impl Hamiltonian {
    /// Builds a nearest-neighbour transverse-field Ising Hamiltonian.
    ///
    /// The convention is
    /// `H = -J sum_<ij> sigma_z(i)sigma_z(j) - h sum_i sigma_x(i)`.
    /// Nearest neighbours are derived from unit squared distance.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite coefficients or invalid geometry-based
    /// lattice construction.
    pub fn tfim(
        geometry: &Geometry,
        coupling_j: f64,
        transverse_field_h: f64,
    ) -> Result<Self, HamiltonianError> {
        if !coupling_j.is_finite() {
            return Err(HamiltonianError::NonFiniteCoefficient {
                coefficient: coupling_j,
            });
        }

        if !transverse_field_h.is_finite() {
            return Err(HamiltonianError::NonFiniteCoefficient {
                coefficient: transverse_field_h,
            });
        }

        let lattice = Lattice::from_geometry(geometry, PairSelection::NearestNeighbour)
            .map_err(|error| HamiltonianError::LatticeError(error.to_string()))?;

        let num_sites = lattice.num_sites();

        let mut terms = Vec::with_capacity(lattice.num_bonds() + num_sites);

        for bond in lattice.bonds() {
            let (site_i, site_j) = bond.sites();

            terms.push(HamiltonianTerm::new(
                TermSupport::Pair(site_i, site_j),
                LocalOperatorKind::SpinZZ,
                -coupling_j,
            )?);
        }

        for site in 0..num_sites {
            terms.push(HamiltonianTerm::new(
                TermSupport::Site(site as u32),
                LocalOperatorKind::SpinX,
                -transverse_field_h,
            )?);
        }

        Self::new(num_sites, terms)
    }
}

impl Hamiltonian {
    /// Builds a long-range Rydberg Hamiltonian on every unordered site pair.
    ///
    /// The convention is `H = -(omega/2) sum_i sigma_x(i) - detuning sum_i
    /// n(i) + sum_{i<j} c6/r_ij^6 n(i)n(j)`.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite coefficients, invalid site distances,
    /// or geometry distance failures.
    pub fn rydberg(
        geometry: &Geometry,
        omega: f64,
        detuning: f64,
        c6: f64,
    ) -> Result<Self, HamiltonianError> {
        for coefficient in [omega, detuning, c6] {
            if !coefficient.is_finite() {
                return Err(HamiltonianError::NonFiniteCoefficient { coefficient });
            }
        }

        let num_sites = geometry.num_sites();
        let all_pairs = geometry.all_pairs();

        let mut terms = Vec::with_capacity(2 * num_sites + all_pairs.len());

        for site in 0..num_sites {
            let site = site as u32;

            terms.push(HamiltonianTerm::new(
                TermSupport::Site(site),
                LocalOperatorKind::SpinX,
                -0.5 * omega,
            )?);

            terms.push(HamiltonianTerm::new(
                TermSupport::Site(site),
                LocalOperatorKind::Number,
                -detuning,
            )?);
        }

        for (site_i, site_j) in all_pairs {
            let distance = geometry
                .distance(site_i, site_j)
                .map_err(|error| HamiltonianError::GeometryError(error.to_string()))?;

            if !distance.is_finite() || distance <= 0.0 {
                return Err(HamiltonianError::InvalidRydbergDistance {
                    site_i,
                    site_j,
                    distance,
                });
            }

            let interaction = c6 / distance.powi(6);

            terms.push(HamiltonianTerm::new(
                TermSupport::Pair(site_i, site_j),
                LocalOperatorKind::NumberNumber,
                interaction,
            )?);
        }

        Self::new(num_sites, terms)
    }
}

impl Hamiltonian {
    /// Builds a diagonal J1-J2 Ising Hamiltonian.
    ///
    /// Nearest-neighbour and diagonal next-nearest-neighbour pairs receive
    /// coefficients `j1` and `j2`, respectively.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite coefficients or failed bond generation.
    pub fn j1_j2_ising(geometry: &Geometry, j1: f64, j2: f64) -> Result<Self, HamiltonianError> {
        for coefficient in [j1, j2] {
            if !coefficient.is_finite() {
                return Err(HamiltonianError::NonFiniteCoefficient { coefficient });
            }
        }

        let nn = Lattice::from_geometry(geometry, PairSelection::NearestNeighbour)
            .map_err(|error| HamiltonianError::LatticeError(error.to_string()))?;

        let nnn = Lattice::from_geometry(geometry, PairSelection::NextNearestNeighbour)
            .map_err(|error| HamiltonianError::LatticeError(error.to_string()))?;

        let mut terms = Vec::with_capacity(nn.num_bonds() + nnn.num_bonds());

        for bond in nn.bonds() {
            let (site_i, site_j) = bond.sites();

            terms.push(HamiltonianTerm::new(
                TermSupport::Pair(site_i, site_j),
                LocalOperatorKind::SpinZZ,
                j1,
            )?);
        }

        for bond in nnn.bonds() {
            let (site_i, site_j) = bond.sites();

            terms.push(HamiltonianTerm::new(
                TermSupport::Pair(site_i, site_j),
                LocalOperatorKind::SpinZZ,
                j2,
            )?);
        }

        Self::new(geometry.num_sites(), terms)
    }
}
