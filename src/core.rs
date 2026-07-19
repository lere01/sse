//! Fundamental operator and spin labels shared by the SSE implementation.

/// Classification of an entry in a fixed-length SSE operator string.
///
/// The explicit representation makes the enum safe to store in compact
/// diagnostic formats and preserves a stable ordering for identity, diagonal,
/// and off-diagonal vertices.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatorKind {
    /// An unused position in the padded operator string.
    Identity = 0,
    /// A local operator diagonal in the sampled spin basis.
    Diagonal = 1,
    /// A local operator that changes the sampled spin basis state.
    OffDiagonal = 2,
}

/// A spin-1/2 basis value in the Pauli-z convention.
///
/// `Up` and `Down` have eigenvalues `+1` and `-1`, respectively. For Rydberg
/// models, the same values represent occupied and empty sites.
#[repr(i8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spin {
    /// The `+1` Pauli-z eigenstate, interpreted as occupied by Rydberg models.
    Up = 1,
    /// The `-1` Pauli-z eigenstate, interpreted as empty by Rydberg models.
    Down = -1,
}

impl Spin {
    /// Returns the opposite spin value.
    #[must_use]
    pub fn flip(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
        }
    }

    /// Returns the Pauli-z eigenvalue, either `+1` or `-1`.
    #[must_use]
    pub fn value(self) -> i8 {
        self as i8
    }

    /// Returns the Rydberg occupation number associated with this spin.
    ///
    /// The convention is `Down -> 0` and `Up -> 1`.
    #[must_use]
    pub fn occupation(self) -> f64 {
        match self {
            Self::Down => 0.0,
            Self::Up => 1.0,
        }
    }
}
