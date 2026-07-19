//! Validated one- and two-dimensional simulation geometries.
//!
//! Geometry owns only spatial information. Bond selection is implemented by
//! [`crate::Lattice`], while Hamiltonian-specific coefficients live in
//! [`crate::Hamiltonian`] or [`crate::LocalSseModel`].

use std::error::Error;
use std::fmt;

/// Boundary condition applied along a lattice direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryCondition {
    /// Sites do not wrap across the boundary.
    Open,
    /// Sites wrap with the minimum-image convention.
    Periodic,
}

/// Compact geometric description with boundary-aware distance operations.
///
/// Site IDs are contiguous `u32` values in `0..num_sites()`. Chains use unit
/// spacing on the x axis. Rectangular lattices use row-major indexing
/// `site = x + lx * y`. Custom coordinates currently have open boundaries.
#[derive(Debug)]
pub enum Geometry {
    /// A unit-spaced one-dimensional chain embedded on the x axis.
    Chain {
        /// Number of sites in the chain.
        length: usize,
        /// Boundary condition along the chain.
        boundary: BoundaryCondition,
    },

    /// A unit-spaced rectangular lattice with independently configured axes.
    Rectangular {
        /// Number of sites along the x axis.
        lx: usize,
        /// Number of sites along the y axis.
        ly: usize,
        /// Boundary condition along the x axis.
        boundary_x: BoundaryCondition,
        /// Boundary condition along the y axis.
        boundary_y: BoundaryCondition,
    },

    /// Arbitrary sites embedded in two-dimensional space.
    ///
    /// This first implementation treats custom geometries as open:
    /// no periodic simulation cell is assumed.
    Custom {
        /// Cartesian coordinates indexed by site ID.
        coordinates: Vec<[f64; 2]>,
    },
}

/// Failure while constructing or querying a [`Geometry`].
#[derive(Clone, Debug, PartialEq)]
pub enum GeometryError {
    /// A chain was requested with no sites.
    ZeroLength,
    /// A named rectangular dimension was zero.
    ZeroDimension {
        /// Name of the zero-valued dimension, currently `"lx"` or `"ly"`.
        dimension: &'static str,
    },
    /// Multiplying rectangular dimensions overflowed `usize`.
    SiteCountOverflow,
    /// The number of sites cannot be represented by the crate's `u32` IDs.
    TooManySites {
        /// Requested number of sites.
        num_sites: usize,
    },
    /// A site ID lies outside the geometry.
    InvalidSite {
        /// Invalid site ID.
        site: u32,
        /// Number of valid sites.
        num_sites: usize,
    },
    /// An `(x, y)` coordinate lies outside a rectangular geometry.
    InvalidRectangularCoordinate {
        /// Requested x coordinate.
        x: usize,
        /// Requested y coordinate.
        y: usize,
        /// Rectangular x extent.
        lx: usize,
        /// Rectangular y extent.
        ly: usize,
    },
    /// A rectangular-only operation was called on another geometry variant.
    NotRectangular,
    /// A custom geometry was constructed without any coordinates.
    EmptyCustomGeometry,
    /// A custom coordinate contained NaN or infinity.
    NonFiniteCoordinate {
        /// Zero-based site index of the invalid coordinate.
        site: usize,
        /// Invalid coordinate value.
        coordinate: [f64; 2],
    },
    /// A distance comparison tolerance was negative or non-finite.
    InvalidTolerance {
        /// Invalid tolerance value.
        tolerance: f64,
    },
    /// A target squared distance was negative or non-finite.
    InvalidTargetDistanceSquared {
        /// Invalid squared distance.
        distance_squared: f64,
    },
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLength => {
                write!(f, "chain length must be greater than zero")
            }

            Self::ZeroDimension { dimension } => {
                write!(f, "{dimension} must be greater than zero")
            }

            Self::SiteCountOverflow => {
                write!(f, "lattice site count overflowed usize")
            }

            Self::TooManySites { num_sites } => {
                write!(
                    f,
                    "geometry has {num_sites} sites, but site IDs are stored as u32"
                )
            }

            Self::InvalidSite { site, num_sites } => {
                write!(
                    f,
                    "site index {site} is invalid for a geometry with {num_sites} sites"
                )
            }

            Self::InvalidRectangularCoordinate { x, y, lx, ly } => {
                write!(
                    f,
                    "coordinate ({x}, {y}) is outside rectangular geometry {lx} × {ly}"
                )
            }

            Self::NotRectangular => {
                write!(
                    f,
                    "this operation is only defined for rectangular geometries"
                )
            }
            Self::EmptyCustomGeometry => {
                write!(f, "custom geometry must contain at least one site")
            }
            Self::NonFiniteCoordinate { site, coordinate } => {
                write!(
                    f,
                    "custom site {site} has a non-finite coordinate: {coordinate:?}"
                )
            }
            Self::InvalidTolerance { tolerance } => {
                write!(
                    f,
                    "distance tolerance must be finite and non-negative; got {tolerance}"
                )
            }
            Self::InvalidTargetDistanceSquared { distance_squared } => {
                write!(
                    f,
                    "target squared distance must be finite and non-negative; \
                     got {distance_squared}"
                )
            }
        }
    }
}

impl Error for GeometryError {}

impl Geometry {
    // ---------------------------------------------------------------------
    // Validated constructors
    // ---------------------------------------------------------------------
    /// Constructs a validated unit-spaced chain.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::ZeroLength`] for an empty chain and
    /// [`GeometryError::TooManySites`] if site IDs would not fit in `u32`.
    pub fn chain(length: usize, boundary: BoundaryCondition) -> Result<Self, GeometryError> {
        let geometry = Self::Chain { length, boundary };
        geometry.validate()?;
        Ok(geometry)
    }

    /// Constructs a validated unit-spaced rectangular geometry.
    ///
    /// # Errors
    ///
    /// Returns an error for zero dimensions, site-count overflow, or a site
    /// count that cannot be represented with `u32` IDs.
    pub fn rectangular(
        lx: usize,
        ly: usize,
        boundary_x: BoundaryCondition,
        boundary_y: BoundaryCondition,
    ) -> Result<Self, GeometryError> {
        let geometry = Self::Rectangular {
            lx,
            ly,
            boundary_x,
            boundary_y,
        };
        geometry.validate()?;
        Ok(geometry)
    }

    /// Constructs a square geometry using one boundary condition on both axes.
    ///
    /// This is equivalent to `Geometry::rectangular(length, length, boundary,
    /// boundary)`.
    pub fn square(length: usize, boundary: BoundaryCondition) -> Result<Self, GeometryError> {
        Self::rectangular(length, length, boundary, boundary)
    }

    /// Constructs an open geometry from arbitrary two-dimensional coordinates.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection is empty, any coordinate is
    /// non-finite, or the number of coordinates exceeds the `u32` site range.
    pub fn custom(coordinates: Vec<[f64; 2]>) -> Result<Self, GeometryError> {
        let geometry = Self::Custom { coordinates };
        geometry.validate()?;
        Ok(geometry)
    }

    // ---------------------------------------------------------------------
    // Validation
    // ---------------------------------------------------------------------
    /// Checks all invariants required by geometry query methods.
    ///
    /// Constructors call this method automatically. It remains public so a
    /// directly constructed enum variant can be validated before use.
    pub fn validate(&self) -> Result<(), GeometryError> {
        let num_sites = match self {
            Self::Chain { length, .. } => {
                if *length == 0 {
                    return Err(GeometryError::ZeroLength);
                }
                *length
            }
            Self::Rectangular { lx, ly, .. } => {
                if *lx == 0 {
                    return Err(GeometryError::ZeroDimension { dimension: "lx" });
                }
                if *ly == 0 {
                    return Err(GeometryError::ZeroDimension { dimension: "ly" });
                }
                lx.checked_mul(*ly)
                    .ok_or(GeometryError::SiteCountOverflow)?
            }

            Self::Custom { coordinates } => {
                if coordinates.is_empty() {
                    return Err(GeometryError::EmptyCustomGeometry);
                }

                for (site, &coordinate) in coordinates.iter().enumerate() {
                    if !coordinate[0].is_finite() || !coordinate[1].is_finite() {
                        return Err(GeometryError::NonFiniteCoordinate { site, coordinate });
                    }
                }
                coordinates.len()
            }
        };

        if num_sites > u32::MAX as usize {
            return Err(GeometryError::TooManySites { num_sites });
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Basic geometry information
    // ---------------------------------------------------------------------
    /// Returns the number of sites represented by the geometry.
    #[must_use]
    pub fn num_sites(&self) -> usize {
        match self {
            Self::Chain { length, .. } => *length,
            Self::Rectangular { lx, ly, .. } => {
                // Constructors and validate() ensure this multiplication
                // does not overflow.
                lx * ly
            }
            Self::Custom { coordinates } => coordinates.len(),
        }
    }

    /// Returns site IDs `0, 1, ..., N - 1` without allocating a Vec.
    #[must_use]
    pub fn sites(&self) -> std::ops::Range<u32> {
        0..self.num_sites() as u32
    }

    /// Reports whether `site` is a valid site ID.
    #[must_use]
    pub fn contains_site(&self, site: u32) -> bool {
        (site as usize) < self.num_sites()
    }

    /// Validates a site ID and returns its equivalent `usize` index.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvalidSite`] when `site >= num_sites()`.
    pub fn validate_site(&self, site: u32) -> Result<usize, GeometryError> {
        let index = site as usize;
        let num_sites = self.num_sites();
        if index >= num_sites {
            return Err(GeometryError::InvalidSite { site, num_sites });
        }
        Ok(index)
    }

    // ---------------------------------------------------------------------
    // Site positions
    // ---------------------------------------------------------------------
    /// Returns the physical position of a site.
    ///
    /// A chain is embedded along the x axis:
    ///
    /// ```text
    /// site i -> [i, 0]
    /// ```
    ///
    /// A rectangular lattice uses unit spacing:
    ///
    /// ```text
    /// site (x, y) -> [x, y]
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvalidSite`] for an out-of-range site ID.
    pub fn position(&self, site: u32) -> Result<[f64; 2], GeometryError> {
        let site = self.validate_site(site)?;

        match self {
            Self::Chain { .. } => Ok([site as f64, 0.0]),
            Self::Rectangular { lx, .. } => {
                let x = site % lx;
                let y = site / lx;
                Ok([x as f64, y as f64])
            }
            Self::Custom { coordinates } => Ok(coordinates[site]),
        }
    }

    // ---------------------------------------------------------------------
    // Rectangular indexing
    // ---------------------------------------------------------------------
    /// Maps `(x, y)` to the flattened site index
    ///
    /// ```text
    /// site = x + lx * y
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NotRectangular`] for non-rectangular geometry,
    /// or [`GeometryError::InvalidRectangularCoordinate`] outside its bounds.
    pub fn rectangular_site_index(&self, x: usize, y: usize) -> Result<u32, GeometryError> {
        let Self::Rectangular { lx, ly, .. } = self else {
            return Err(GeometryError::NotRectangular);
        };
        if x >= *lx || y >= *ly {
            return Err(GeometryError::InvalidRectangularCoordinate {
                x,
                y,
                lx: *lx,
                ly: *ly,
            });
        }
        Ok((x + lx * y) as u32)
    }

    /// Maps a flattened site index back to `(x, y)`.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid site or a non-rectangular geometry.
    pub fn rectangular_coordinates(&self, site: u32) -> Result<(usize, usize), GeometryError> {
        let site = self.validate_site(site)?;
        let Self::Rectangular { lx, .. } = self else {
            return Err(GeometryError::NotRectangular);
        };
        let x = site % lx;
        let y = site / lx;
        Ok((x, y))
    }
    // ---------------------------------------------------------------------
    // Boundary-aware displacement and distance
    // ---------------------------------------------------------------------
    /// Returns the minimum-image displacement from site i to site j.
    ///
    /// The result is:
    ///
    /// ```text
    /// r_j - r_i
    /// ```
    ///
    /// Periodic axes are reduced with the minimum-image convention. Custom
    /// coordinates use their direct Euclidean displacement.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvalidSite`] if either site is invalid.
    pub fn displacement(&self, site_i: u32, site_j: u32) -> Result<[f64; 2], GeometryError> {
        let position_i = self.position(site_i)?;
        let position_j = self.position(site_j)?;
        let mut dx = position_j[0] - position_i[0];
        let mut dy = position_j[1] - position_i[1];
        match self {
            Self::Chain { length, boundary } => {
                if *boundary == BoundaryCondition::Periodic {
                    dx = minimum_image(dx, *length as f64);
                }

                // All sites lie along the x axis.
                dy = 0.0;
            }

            Self::Rectangular {
                lx,
                ly,
                boundary_x,
                boundary_y,
            } => {
                if *boundary_x == BoundaryCondition::Periodic {
                    dx = minimum_image(dx, *lx as f64);
                }

                if *boundary_y == BoundaryCondition::Periodic {
                    dy = minimum_image(dy, *ly as f64);
                }
            }

            Self::Custom { .. } => {
                // Custom coordinates currently use ordinary open Euclidean
                // displacement. A periodic custom simulation cell can be
                // added later as another Geometry variant.
            }
        }
        Ok([dx, dy])
    }

    /// Returns the squared boundary-aware Euclidean distance between two sites.
    ///
    /// Squared distances avoid a square root and are preferred for selecting
    /// lattice shells.
    pub fn distance_squared(&self, site_i: u32, site_j: u32) -> Result<f64, GeometryError> {
        let [dx, dy] = self.displacement(site_i, site_j)?;
        Ok(dx * dx + dy * dy)
    }

    /// Returns the boundary-aware Euclidean distance between two sites.
    pub fn distance(&self, site_i: u32, site_j: u32) -> Result<f64, GeometryError> {
        Ok(self.distance_squared(site_i, site_j)?.sqrt())
    }

    // ---------------------------------------------------------------------
    // Pair generation
    // ---------------------------------------------------------------------
    /// Generates every unordered pair `(i, j)` with `i < j`.
    ///
    /// This is O(N²) in both time and output size.
    #[must_use]
    pub fn all_pairs(&self) -> Vec<(u32, u32)> {
        let num_sites = self.num_sites();

        let pair_count = num_sites
            .checked_mul(num_sites.saturating_sub(1))
            .and_then(|value| value.checked_div(2))
            .unwrap_or(0);

        let mut pairs = Vec::with_capacity(pair_count);

        for i in 0..num_sites {
            for j in (i + 1)..num_sites {
                pairs.push((i as u32, j as u32));
            }
        }
        pairs
    }

    /// Selects unordered site pairs whose squared distance is close to
    /// `target_distance_squared`.
    ///
    /// For a unit-spaced rectangular lattice:
    ///
    /// - nearest neighbours: target = 1
    /// - diagonal next-nearest neighbours: target = 2
    /// - axial next-next-nearest neighbours: target = 4
    ///
    /// Each pair is returned exactly once with `i < j`. The comparison is
    /// absolute: `|distance_squared - target| <= tolerance`.
    ///
    /// # Errors
    ///
    /// Returns an error for negative or non-finite target/tolerance values, or
    /// if a generated site pair cannot be evaluated.
    pub fn pairs_at_distance_squared(
        &self,
        target_distance_squared: f64,
        tolerance: f64,
    ) -> Result<Vec<(u32, u32)>, GeometryError> {
        if !target_distance_squared.is_finite() || target_distance_squared < 0.0 {
            return Err(GeometryError::InvalidTargetDistanceSquared {
                distance_squared: target_distance_squared,
            });
        }

        if !tolerance.is_finite() || tolerance < 0.0 {
            return Err(GeometryError::InvalidTolerance { tolerance });
        }

        let mut pairs = Vec::new();
        for (i, j) in self.all_pairs() {
            let distance_squared = self.distance_squared(i, j)?;

            if (distance_squared - target_distance_squared).abs() <= tolerance {
                pairs.push((i, j));
            }
        }

        Ok(pairs)
    }
}

/// Maps a displacement into the interval approximately `[-L/2, L/2]`.
///
/// This implements the minimum-image convention for a periodic direction.
fn minimum_image(displacement: f64, length: f64) -> f64 {
    debug_assert!(length > 0.0);
    displacement - length * (displacement / length).round()
}
