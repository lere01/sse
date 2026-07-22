//! Resolution of configured geometries and Hamiltonians onto qslib types.
//!
//! The configuration schema stays geometry-driven; this module converts it to
//! the canonical `qslib` vocabulary: row-major rectangular or custom
//! coordinate geometries, explicit weighted pair lists, and the sign-safe
//! `LocalSseModel` decompositions.

use qslib::sse::{convert_legacy_bits, LegacyModelKind, LegacySpin, LocalSseModel};
use qslib::{
    BasisBit, Boundary, Coordinate, CustomGeometry, RectangularGeometry, ShellTolerance, SiteId,
};

/// Nearest-neighbour shell tolerance shared by every geometry backend.
const SHELL_TOLERANCE: f64 = 1.0e-12;

/// A validated qslib geometry resolved from the configuration schema.
#[derive(Clone, Debug)]
pub enum ResolvedGeometry {
    /// Chains and rectangular lattices with per-axis boundaries.
    Rectangular(RectangularGeometry),
    /// Arbitrary open two-dimensional coordinates.
    Custom(CustomGeometry),
}

impl ResolvedGeometry {
    /// Builds a unit-spaced chain as an `length x 1` rectangular geometry.
    pub fn chain(length: usize, boundary: Boundary) -> Result<Self, String> {
        RectangularGeometry::new(length, 1, boundary, Boundary::Open)
            .map(Self::Rectangular)
            .map_err(|error| error.to_string())
    }

    /// Builds a unit-spaced rectangular lattice.
    pub fn rectangular(
        lx: usize,
        ly: usize,
        boundary_x: Boundary,
        boundary_y: Boundary,
    ) -> Result<Self, String> {
        RectangularGeometry::new(lx, ly, boundary_x, boundary_y)
            .map(Self::Rectangular)
            .map_err(|error| error.to_string())
    }

    /// Builds an open custom geometry from explicit coordinates.
    pub fn custom(coordinates: &[[f64; 2]]) -> Result<Self, String> {
        let coordinates = coordinates
            .iter()
            .map(|&[x, y]| Coordinate::new(x, y).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        CustomGeometry::new(coordinates)
            .map(Self::Custom)
            .map_err(|error| error.to_string())
    }

    /// Returns the number of sites.
    #[must_use]
    pub fn num_sites(&self) -> usize {
        match self {
            Self::Rectangular(geometry) => geometry.site_count().get(),
            Self::Custom(geometry) => geometry.site_count().get(),
        }
    }

    /// Returns unordered pairs at unit squared distance.
    ///
    /// Periodic axes use the minimum-image convention, matching the canonical
    /// qslib shell selection.
    pub fn nearest_neighbour_pairs(&self) -> Result<Vec<(u32, u32)>, String> {
        match self {
            Self::Rectangular(geometry) => Ok(geometry
                .pairs_at_squared_distance(1.0, ShellTolerance::Absolute(SHELL_TOLERANCE))
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|bond| (bond.first().get(), bond.second().get()))
                .collect()),
            Self::Custom(_) => Ok(self
                .all_pairs_squared_distances()?
                .into_iter()
                .filter(|&(_, _, squared)| (squared - 1.0).abs() <= SHELL_TOLERANCE)
                .map(|(first, second, _)| (first, second))
                .collect()),
        }
    }

    /// Returns every unordered pair with its squared distance.
    pub fn all_pairs_squared_distances(&self) -> Result<Vec<(u32, u32, f64)>, String> {
        let num_sites = self.num_sites() as u32;
        let mut pairs = Vec::with_capacity((num_sites as usize * (num_sites as usize - 1)) / 2);
        for first in 0..num_sites {
            for second in (first + 1)..num_sites {
                let squared = self.squared_distance(first, second)?;
                pairs.push((first, second, squared));
            }
        }
        Ok(pairs)
    }

    fn squared_distance(&self, first: u32, second: u32) -> Result<f64, String> {
        match self {
            Self::Rectangular(geometry) => {
                let displacement = geometry
                    .minimum_image_displacement(SiteId::new(first), SiteId::new(second))
                    .map_err(|error| error.to_string())?;
                Ok(displacement.x() * displacement.x() + displacement.y() * displacement.y())
            }
            Self::Custom(geometry) => {
                let from = geometry
                    .coordinate(SiteId::new(first))
                    .map_err(|error| error.to_string())?;
                let to = geometry
                    .coordinate(SiteId::new(second))
                    .map_err(|error| error.to_string())?;
                let dx = to.x() - from.x();
                let dy = to.y() - from.y();
                Ok(dx * dx + dy * dy)
            }
        }
    }
}

/// Builds the sign-safe TFIM decomposition with unit-distance bonds.
pub fn build_tfim(
    geometry: &ResolvedGeometry,
    coupling: f64,
    field: f64,
) -> Result<LocalSseModel, String> {
    let bonds = geometry.nearest_neighbour_pairs()?;
    LocalSseModel::tfim(geometry.num_sites(), &bonds, coupling, field)
        .map_err(|error| error.to_string())
}

/// Builds the sign-safe Rydberg decomposition with `C6 / r^6` pair couplings.
pub fn build_rydberg(
    geometry: &ResolvedGeometry,
    omega: f64,
    detuning: f64,
    c6: f64,
) -> Result<LocalSseModel, String> {
    let num_sites = geometry.num_sites();
    let mut interactions = Vec::new();
    for (first, second, squared) in geometry.all_pairs_squared_distances()? {
        if squared <= 0.0 || !squared.is_finite() {
            return Err(format!(
                "sites {first} and {second} must be separated by a positive distance"
            ));
        }
        interactions.push((first, second, c6 / squared.powi(3)));
    }
    LocalSseModel::rydberg(num_sites, &vec![detuning; num_sites], &interactions, omega)
        .map_err(|error| error.to_string())
}

/// Materializes the shared initial basis state for a model family.
///
/// Legacy `up`/`down` labels are model-dependent; the explicit qslib adapter
/// maps them onto canonical bits (TFIM: up is `+Z`; Rydberg: up is occupied).
#[must_use]
pub fn initial_bits(kind: LegacyModelKind, spins: &[LegacySpin]) -> Vec<BasisBit> {
    convert_legacy_bits(kind, spins)
}
