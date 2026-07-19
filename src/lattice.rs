//! Bond topology derived from geometry or explicit site pairs.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use crate::geometry::Geometry;

/// Rule used to select unordered bonds from a [`Geometry`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PairSelection {
    /// All pairs at squared distance 1.
    NearestNeighbour,

    /// All pairs at squared distance 2.
    ///
    /// For a unit-spaced rectangular lattice, these are diagonal neighbours.
    NextNearestNeighbour,

    /// All pairs at squared distance 4.
    ///
    /// For a unit-spaced rectangular lattice, these are two lattice
    /// spacings apart along an axis.
    NextNextNearestNeighbour,

    /// All unordered pairs i < j.
    AllToAll,

    /// All pairs at a user-specified squared distance.
    DistanceSquared {
        /// Target squared distance.
        value: f64,
        /// Maximum absolute difference from `value`.
        tolerance: f64,
    },
}

/// Failure while building a validated [`Lattice`].
#[derive(Debug)]
pub enum LatticeError {
    /// A lattice was requested with zero sites.
    EmptyLattice,
    /// A bond endpoint is outside the lattice.
    InvalidSite {
        /// Invalid endpoint.
        site: u32,
        /// Number of sites in the lattice.
        num_sites: usize,
    },
    /// A bond connected a site to itself.
    SelfBond {
        /// Repeated endpoint.
        site: u32,
    },
    /// The input contained the same unordered bond more than once.
    DuplicateBond {
        /// Canonically ordered duplicate endpoints.
        sites: (u32, u32),
    },
    /// Geometry-based pair generation failed.
    GeometryError(String),
}

impl fmt::Display for LatticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyLattice => {
                write!(f, "a lattice must contain at least one site")
            }

            Self::InvalidSite { site, num_sites } => {
                write!(
                    f,
                    "site {site} is invalid for a lattice with \
                     {num_sites} sites"
                )
            }

            Self::SelfBond { site } => {
                write!(f, "a bond cannot connect site {site} to itself")
            }

            Self::DuplicateBond { sites } => {
                write!(
                    f,
                    "duplicate bond detected between sites {} and {}",
                    sites.0, sites.1
                )
            }

            Self::GeometryError(message) => {
                write!(f, "geometry error: {message}")
            }
        }
    }
}

impl Error for LatticeError {}

/// A validated collection of unique, unordered bonds over a fixed site set.
///
/// Bond endpoints are canonicalized so the smaller site ID is always first.
#[derive(Debug)]
pub struct Lattice {
    bonds: Vec<Bond>,
    num_sites: usize,
}

impl Lattice {
    /// Generates a lattice by applying `pair_selection` to `geometry`.
    ///
    /// # Errors
    ///
    /// Returns an error if geometry pair generation fails or if the resulting
    /// sites and bonds violate lattice invariants.
    pub fn from_geometry(
        geometry: &Geometry,
        pair_selection: PairSelection,
    ) -> Result<Self, LatticeError> {
        let num_sites = geometry.num_sites();

        if num_sites == 0 {
            return Err(LatticeError::EmptyLattice);
        }

        let pairs = match pair_selection {
            PairSelection::NearestNeighbour => geometry
                .pairs_at_distance_squared(1.0, 1.0e-12)
                .map_err(|error| LatticeError::GeometryError(error.to_string()))?,

            PairSelection::NextNearestNeighbour => geometry
                .pairs_at_distance_squared(2.0, 1.0e-12)
                .map_err(|error| LatticeError::GeometryError(error.to_string()))?,

            PairSelection::NextNextNearestNeighbour => geometry
                .pairs_at_distance_squared(4.0, 1.0e-12)
                .map_err(|error| LatticeError::GeometryError(error.to_string()))?,

            PairSelection::AllToAll => geometry.all_pairs(),

            PairSelection::DistanceSquared { value, tolerance } => geometry
                .pairs_at_distance_squared(value, tolerance)
                .map_err(|error| LatticeError::GeometryError(error.to_string()))?,
        };

        Self::from_pairs(num_sites, pairs)
    }

    /// Constructs a lattice from explicit unordered site pairs.
    ///
    /// Input pairs may use either endpoint order. Each stored [`Bond`] is
    /// canonicalized, and reversed duplicates are rejected.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty site set, invalid endpoint, self-bond, or
    /// duplicate unordered bond.
    pub fn from_pairs(num_sites: usize, pairs: Vec<(u32, u32)>) -> Result<Self, LatticeError> {
        if num_sites == 0 {
            return Err(LatticeError::EmptyLattice);
        }

        let mut bonds = Vec::with_capacity(pairs.len());
        let mut seen = HashSet::with_capacity(pairs.len());

        for (site_i, site_j) in pairs {
            Self::validate_site(site_i, num_sites)?;
            Self::validate_site(site_j, num_sites)?;

            let bond = Bond::new(site_i, site_j)?;

            if !seen.insert(bond.sites()) {
                return Err(LatticeError::DuplicateBond {
                    sites: bond.sites(),
                });
            }

            bonds.push(bond);
        }

        Ok(Self { num_sites, bonds })
    }

    fn validate_site(site: u32, num_sites: usize) -> Result<(), LatticeError> {
        if site as usize >= num_sites {
            return Err(LatticeError::InvalidSite { site, num_sites });
        }

        Ok(())
    }

    /// Returns the number of sites, including sites with no bonds.
    #[must_use]
    pub fn num_sites(&self) -> usize {
        self.num_sites
    }

    /// Returns the number of bonds.
    #[must_use]
    pub fn num_bonds(&self) -> usize {
        self.bonds.len()
    }

    /// Borrows all bonds in construction order.
    #[must_use]
    pub fn bonds(&self) -> &[Bond] {
        &self.bonds
    }

    /// Borrows a bond by zero-based index, or returns `None` if out of range.
    #[must_use]
    pub fn bond(&self, bond_index: usize) -> Option<&Bond> {
        self.bonds.get(bond_index)
    }
}

/// An unordered, non-self bond stored in canonical endpoint order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bond {
    /// Endpoints as `(smaller_site, larger_site)`.
    pub sites: (u32, u32),
}

impl Bond {
    /// Creates a bond and canonicalizes the endpoint order.
    ///
    /// # Errors
    ///
    /// Returns [`LatticeError::SelfBond`] when both endpoints are equal.
    pub fn new(site_i: u32, site_j: u32) -> Result<Self, LatticeError> {
        if site_i == site_j {
            return Err(LatticeError::SelfBond { site: site_i });
        }

        // Store every bond canonically as (smaller, larger).
        let sites = if site_i < site_j {
            (site_i, site_j)
        } else {
            (site_j, site_i)
        };

        Ok(Self { sites })
    }

    /// Returns both endpoints in canonical order.
    #[must_use]
    pub fn sites(&self) -> (u32, u32) {
        self.sites
    }

    /// Returns the smaller endpoint.
    #[must_use]
    pub fn first_site(&self) -> u32 {
        self.sites.0
    }

    /// Returns the larger endpoint.
    #[must_use]
    pub fn second_site(&self) -> u32 {
        self.sites.1
    }
}
