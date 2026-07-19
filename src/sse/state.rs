//! Basis state, padded operator string, and trace propagation.

use crate::core::{OperatorKind, Spin};
use crate::sse::{SseModel, SseModelError};

const NO_TERM: u32 = u32::MAX;

/// One position in a padded SSE operator string.
///
/// Identity entries use an internal sentinel term index. For non-identity
/// entries, `term_index` addresses the corresponding [`SseModel`] term.
#[derive(Clone, Copy, Debug)]
pub struct Operator {
    /// Identity, diagonal, or off-diagonal classification.
    pub kind: OperatorKind,
    /// Model term index, or an internal sentinel for an identity entry.
    pub term_index: u32,
}

impl Operator {
    /// Constructs an unused identity entry.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            kind: OperatorKind::Identity,
            term_index: NO_TERM,
        }
    }

    /// Constructs a diagonal entry referring to `term_index`.
    #[must_use]
    pub fn diagonal(term_index: u32) -> Self {
        Self {
            kind: OperatorKind::Diagonal,
            term_index,
        }
    }

    /// Constructs an off-diagonal entry referring to `term_index`.
    #[must_use]
    pub fn off_diagonal(term_index: u32) -> Self {
        Self {
            kind: OperatorKind::OffDiagonal,
            term_index,
        }
    }

    /// Returns the referenced model index, or `None` for an identity entry.
    #[must_use]
    pub fn term_index(&self) -> Option<usize> {
        match self.kind {
            OperatorKind::Identity => None,
            OperatorKind::Diagonal | OperatorKind::OffDiagonal => Some(self.term_index as usize),
        }
    }
}

/// Mutable Monte Carlo state for a fixed-length SSE representation.
///
/// The state contains the Pauli-z basis state at the imaginary-time boundary
/// and a padded operator string. Valid configurations close the trace when the
/// entire string is propagated.
#[derive(Debug)]
pub struct SSEState {
    pub(super) basis_state: Vec<Spin>,
    pub(super) operator_string: Vec<Operator>,
}

impl SSEState {
    /// Creates an identity-filled operator string around an initial basis state.
    ///
    /// # Errors
    ///
    /// Returns [`SseModelError::InvalidBasisStateLength`] if there is not one
    /// spin per model site, or [`SseModelError::InvalidOperatorStringLength`]
    /// if `operator_string_length` is zero.
    pub fn new<M: SseModel>(
        model: &M,
        basis_state: Vec<Spin>,
        operator_string_length: usize,
    ) -> Result<Self, SseModelError> {
        if basis_state.len() != model.num_sites() {
            return Err(SseModelError::InvalidBasisStateLength {
                received: basis_state.len(),
                expected: model.num_sites(),
            });
        }

        if operator_string_length == 0 {
            return Err(SseModelError::InvalidOperatorStringLength);
        }

        Ok(Self {
            basis_state,
            operator_string: vec![Operator::identity(); operator_string_length],
        })
    }

    /// Borrows the spin state at the imaginary-time boundary.
    #[must_use]
    pub fn basis_state(&self) -> &[Spin] {
        &self.basis_state
    }

    /// Borrows the complete padded operator string.
    #[must_use]
    pub fn operator_string(&self) -> &[Operator] {
        &self.operator_string
    }

    /// Number of non-identity operators in the current string.
    #[must_use]
    pub fn expansion_order(&self) -> usize {
        self.operator_string
            .iter()
            .filter(|operator| operator.kind != OperatorKind::Identity)
            .count()
    }

    /// Returns the current padded operator-string cutoff `M`.
    #[must_use]
    pub fn operator_string_length(&self) -> usize {
        self.operator_string.len()
    }

    /// Increase the fixed-length cutoff while preserving all operators.
    ///
    /// Requests at or below the current length have no effect.
    pub fn grow_operator_string(&mut self, new_length: usize) {
        if new_length > self.operator_string.len() {
            self.operator_string
                .resize(new_length, Operator::identity());
        }
    }
}

/// Result of propagating an [`SSEState`] through its operator string.
#[derive(Debug)]
pub struct PropagationResult {
    /// Basis state after applying every non-identity operator in order.
    pub final_state: Vec<Spin>,

    /// Sum of logarithmic local matrix elements along the propagation path.
    pub log_weight: f64,

    /// Whether `final_state` equals the state's initial boundary state.
    pub trace_closed: bool,
}

impl SSEState {
    /// Propagates the basis state through the complete operator string.
    ///
    /// Identity positions are skipped. Every other position is checked against
    /// the model's operator classification, contributes its log matrix element,
    /// and applies its off-diagonal action when necessary.
    ///
    /// # Errors
    ///
    /// Returns an error for mismatched state size, invalid operator references,
    /// wrong operator kinds, or non-positive matrix elements.
    pub fn propagate<M: SseModel>(&self, model: &M) -> Result<PropagationResult, SseModelError> {
        if self.basis_state.len() != model.num_sites() {
            return Err(SseModelError::InvalidCoefficient {
                name: "basis-state length",

                value: self.basis_state.len() as f64,
            });
        }

        let mut working_state = self.basis_state.clone();

        let mut log_weight = 0.0;

        for (position, operator) in self.operator_string.iter().enumerate() {
            if operator.kind == OperatorKind::Identity {
                continue;
            }

            let term_index =
                operator
                    .term_index()
                    .ok_or(SseModelError::InvalidOperatorReference {
                        position,

                        term_index: operator.term_index,
                    })?;

            let expected_kind = model.operator_kind(term_index)?;

            if operator.kind != expected_kind {
                return Err(SseModelError::InvalidOperatorKind {
                    term_index,

                    expected: expected_kind,

                    received: operator.kind,
                });
            }

            let matrix_element = model.matrix_element(term_index, &working_state)?;

            if matrix_element <= 0.0 {
                return Err(SseModelError::ZeroMatrixElement { term_index });
            }

            log_weight += matrix_element.ln();

            if operator.kind == OperatorKind::OffDiagonal {
                model.apply_off_diagonal(term_index, &mut working_state)?;
            }
        }

        let trace_closed = working_state == self.basis_state;

        Ok(PropagationResult {
            final_state: working_state,

            log_weight,

            trace_closed,
        })
    }

    /// Verifies periodic imaginary-time boundary conditions.
    ///
    /// # Errors
    ///
    /// Propagation errors are forwarded. A valid propagation whose final state
    /// differs from its initial state returns [`SseModelError::TraceNotClosed`].
    pub fn validate_trace<M: SseModel>(&self, model: &M) -> Result<(), SseModelError> {
        let result = self.propagate(model)?;

        if !result.trace_closed {
            return Err(SseModelError::TraceNotClosed);
        }

        Ok(())
    }
}
