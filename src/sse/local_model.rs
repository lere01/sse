//! Built-in sign-safe decompositions for TFIM and Rydberg Hamiltonians.

use crate::core::{OperatorKind, Spin};
use crate::geometry::Geometry;
use crate::sse::{SseModel, SseModelError, SseTerm};

/// Concrete local-term implementation of [`SseModel`].
///
/// Instances are constructed with [`LocalSseModel::tfim`] or
/// [`LocalSseModel::rydberg`]. Each constructor inserts the constant and local
/// shifts required to keep sampled matrix elements non-negative, and records
/// the total in [`SseModel::energy_shift`].
#[derive(Debug)]
pub struct LocalSseModel {
    num_sites: usize,
    terms: Vec<SseTerm>,
    diagonal_term_indices: Vec<u32>,
    transverse_partners: Vec<Option<u32>>,
    family: ModelFamily,
    energy_shift: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelFamily {
    Tfim,
    Rydberg,
}

impl LocalSseModel {
    fn new(
        num_sites: usize,
        terms: Vec<SseTerm>,
        energy_shift: f64,
        family: ModelFamily,
    ) -> Result<Self, SseModelError> {
        if terms.is_empty() {
            return Err(SseModelError::EmptyModel);
        }

        if !energy_shift.is_finite() {
            return Err(SseModelError::InvalidCoefficient {
                name: "energy shift",
                value: energy_shift,
            });
        }

        if terms.len() > u32::MAX as usize {
            return Err(SseModelError::InvalidCoefficient {
                name: "number of terms",
                value: terms.len() as f64,
            });
        }

        for term in &terms {
            match *term {
                SseTerm::SiteConstant { site, amplitude } => {
                    validate_site(site, num_sites)?;
                    validate_nonnegative("site-constant amplitude", amplitude)?;
                }

                SseTerm::TfimBond {
                    site_i,
                    site_j,
                    coupling,
                    shift,
                } => {
                    validate_site(site_i, num_sites)?;
                    validate_site(site_j, num_sites)?;
                    validate_distinct_sites(site_i, site_j)?;

                    validate_nonnegative("TFIM coupling", coupling)?;
                    validate_nonnegative("TFIM diagonal shift", shift)?;
                }

                SseTerm::SpinFlip { site, amplitude } => {
                    validate_site(site, num_sites)?;
                    validate_nonnegative("spin-flip amplitude", amplitude)?;
                }

                SseTerm::RydbergDetuning {
                    site,
                    detuning,
                    shift,
                } => {
                    validate_site(site, num_sites)?;

                    if !detuning.is_finite() {
                        return Err(SseModelError::InvalidCoefficient {
                            name: "Rydberg detuning",
                            value: detuning,
                        });
                    }

                    validate_nonnegative("Rydberg onsite shift", shift)?;
                }

                SseTerm::RydbergInteraction {
                    site_i,
                    site_j,
                    interaction,
                    shift,
                } => {
                    validate_site(site_i, num_sites)?;
                    validate_site(site_j, num_sites)?;
                    validate_distinct_sites(site_i, site_j)?;

                    if !interaction.is_finite() {
                        return Err(SseModelError::InvalidCoefficient {
                            name: "Rydberg interaction",
                            value: interaction,
                        });
                    }

                    validate_nonnegative("Rydberg interaction shift", shift)?;
                }
            }
        }

        let diagonal_term_indices = terms
            .iter()
            .enumerate()
            .filter(|(_, term)| term.operator_kind() == OperatorKind::Diagonal)
            .map(|(index, _)| index as u32)
            .collect();
        let mut transverse_partners = vec![None; terms.len()];
        for (left_index, left) in terms.iter().enumerate() {
            let (left_site, left_amplitude, left_kind) = match *left {
                SseTerm::SiteConstant { site, amplitude } => {
                    (site, amplitude, OperatorKind::Diagonal)
                }
                SseTerm::SpinFlip { site, amplitude } => {
                    (site, amplitude, OperatorKind::OffDiagonal)
                }
                _ => continue,
            };
            for (right_index, right) in terms.iter().enumerate() {
                let (right_site, right_amplitude, right_kind) = match *right {
                    SseTerm::SiteConstant { site, amplitude } => {
                        (site, amplitude, OperatorKind::Diagonal)
                    }
                    SseTerm::SpinFlip { site, amplitude } => {
                        (site, amplitude, OperatorKind::OffDiagonal)
                    }
                    _ => continue,
                };
                if left_site == right_site
                    && left_amplitude == right_amplitude
                    && left_kind != right_kind
                {
                    transverse_partners[left_index] = Some(right_index as u32);
                    break;
                }
            }
        }

        Ok(Self {
            num_sites,
            terms,
            diagonal_term_indices,
            transverse_partners,
            family,
            energy_shift,
        })
    }

    /// Borrows all local SSE terms in their stable index order.
    #[must_use]
    pub fn terms(&self) -> &[SseTerm] {
        &self.terms
    }
}

fn validate_site(site: u32, num_sites: usize) -> Result<(), SseModelError> {
    if site as usize >= num_sites {
        return Err(SseModelError::InvalidSite { site, num_sites });
    }

    Ok(())
}

fn validate_distinct_sites(site_i: u32, site_j: u32) -> Result<(), SseModelError> {
    if site_i == site_j {
        return Err(SseModelError::InvalidCoefficient {
            name: "two-site term with identical endpoints",
            value: site_i as f64,
        });
    }

    Ok(())
}

fn validate_nonnegative(name: &'static str, value: f64) -> Result<(), SseModelError> {
    if !value.is_finite() || value < 0.0 {
        return Err(SseModelError::InvalidCoefficient { name, value });
    }

    Ok(())
}

impl SseModel for LocalSseModel {
    fn num_sites(&self) -> usize {
        self.num_sites
    }

    fn num_terms(&self) -> usize {
        self.terms.len()
    }

    fn energy_shift(&self) -> f64 {
        self.energy_shift
    }

    fn term(&self, term_index: usize) -> Option<&SseTerm> {
        self.terms.get(term_index)
    }

    fn diagonal_term_indices(&self) -> &[u32] {
        &self.diagonal_term_indices
    }

    fn supports_tfim_cluster_update(&self) -> bool {
        self.family == ModelFamily::Tfim
    }

    fn transverse_partner(&self, term_index: usize) -> Option<u32> {
        self.transverse_partners.get(term_index).copied().flatten()
    }

    fn operator_kind(&self, term_index: usize) -> Result<OperatorKind, SseModelError> {
        let term = self
            .terms
            .get(term_index)
            .ok_or(SseModelError::InvalidTermIndex {
                term_index,

                num_terms: self.terms.len(),
            })?;

        Ok(term.operator_kind())
    }

    fn matrix_element(
        &self,
        term_index: usize,
        basis_state: &[Spin],
    ) -> Result<f64, SseModelError> {
        if basis_state.len() != self.num_sites {
            return Err(SseModelError::InvalidCoefficient {
                name: "basis-state length",
                value: basis_state.len() as f64,
            });
        }

        let term = self
            .terms
            .get(term_index)
            .ok_or(SseModelError::InvalidTermIndex {
                term_index,

                num_terms: self.terms.len(),
            })?;

        let matrix_element = match *term {
            SseTerm::SiteConstant { amplitude, .. } => amplitude,

            SseTerm::TfimBond {
                site_i,
                site_j,
                coupling,
                shift,
            } => {
                let spin_i = basis_state[site_i as usize].value() as f64;
                let spin_j = basis_state[site_j as usize].value() as f64;
                coupling * (shift + spin_i * spin_j)
            }

            SseTerm::SpinFlip { amplitude, .. } => {
                // <opposite spin | amplitude σx | spin> = amplitude
                amplitude
            }

            SseTerm::RydbergDetuning {
                site,
                detuning,
                shift,
            } => {
                let occupation = basis_state[site as usize].occupation();
                shift + detuning * occupation
            }

            SseTerm::RydbergInteraction {
                site_i,
                site_j,
                interaction,
                shift,
            } => {
                let occupation_i = basis_state[site_i as usize].occupation();
                let occupation_j = basis_state[site_j as usize].occupation();
                shift - interaction * occupation_i * occupation_j
            }
        };

        if matrix_element < -1.0e-12 {
            return Err(SseModelError::NegativeMatrixElement {
                term_index,
                value: matrix_element,
            });
        }

        // Remove tiny negative values caused by floating-point roundoff.

        Ok(matrix_element.max(0.0))
    }

    fn apply_off_diagonal(
        &self,
        term_index: usize,
        basis_state: &mut [Spin],
    ) -> Result<(), SseModelError> {
        let term = self
            .terms
            .get(term_index)
            .ok_or(SseModelError::InvalidTermIndex {
                term_index,
                num_terms: self.terms.len(),
            })?;

        match *term {
            SseTerm::SpinFlip { site, .. } => {
                let spin = &mut basis_state[site as usize];
                *spin = spin.flip();

                Ok(())
            }

            _ => Err(SseModelError::InvalidOperatorKind {
                term_index,
                expected: OperatorKind::OffDiagonal,
                received: OperatorKind::Diagonal,
            }),
        }
    }
}

impl LocalSseModel {
    /// Constructs a ferromagnetic transverse-field Ising SSE decomposition.
    ///
    /// The represented physical Hamiltonian is
    /// `H = -J sum_(i,j) sigma_z(i)sigma_z(j) - h sum_i sigma_x(i)`.
    /// Every input pair creates one diagonal bond vertex. Every site creates a
    /// matched constant and spin-flip vertex with amplitude `h`.
    ///
    /// # Errors
    ///
    /// Returns an error if `J` or `h` is negative/non-finite, a pair contains
    /// an invalid site, or the generated model violates indexing invariants.
    pub fn tfim(
        geometry: &Geometry,
        nearest_neighbour_pairs: &[(u32, u32)],
        coupling_j: f64,
        transverse_field_h: f64,
    ) -> Result<Self, SseModelError> {
        validate_nonnegative("TFIM J", coupling_j)?;
        validate_nonnegative("TFIM h", transverse_field_h)?;

        let num_sites = geometry.num_sites();

        let mut terms = Vec::with_capacity(nearest_neighbour_pairs.len() + 2 * num_sites);

        let bond_shift = 1.0;

        for &(site_i, site_j) in nearest_neighbour_pairs {
            validate_site(site_i, num_sites)?;
            validate_site(site_j, num_sites)?;

            terms.push(SseTerm::TfimBond {
                site_i,
                site_j,
                coupling: coupling_j,
                shift: bond_shift,
            });
        }

        for site in 0..num_sites {
            terms.push(SseTerm::SiteConstant {
                site: site as u32,
                amplitude: transverse_field_h,
            });

            terms.push(SseTerm::SpinFlip {
                site: site as u32,
                amplitude: transverse_field_h,
            });
        }

        let energy_shift = coupling_j * bond_shift * nearest_neighbour_pairs.len() as f64
            + transverse_field_h * num_sites as f64;

        Self::new(num_sites, terms, energy_shift, ModelFamily::Tfim)
    }
}

impl LocalSseModel {
    /// Constructs a long-range Rydberg SSE decomposition.
    ///
    /// The represented physical Hamiltonian is
    /// `H = -(omega/2) sum_i sigma_x(i) - detuning sum_i n(i)
    /// + sum_(i<j) c6/r_ij^6 n(i)n(j)`.
    /// All unordered geometry pairs are included. The Rabi frequency must be
    /// non-negative, while detuning and `c6` may have either sign.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid coefficients, non-positive pair distances,
    /// geometry failures, or generated terms that violate model invariants.
    pub fn rydberg(
        geometry: &Geometry,
        omega: f64,
        detuning: f64,
        c6: f64,
    ) -> Result<Self, SseModelError> {
        validate_nonnegative("Rabi frequency omega", omega)?;

        if !detuning.is_finite() {
            return Err(SseModelError::InvalidCoefficient {
                name: "Rydberg detuning",
                value: detuning,
            });
        }

        if !c6.is_finite() {
            return Err(SseModelError::InvalidCoefficient {
                name: "Rydberg C6",
                value: c6,
            });
        }

        let num_sites = geometry.num_sites();
        let pairs = geometry.all_pairs();

        let mut terms = Vec::with_capacity(3 * num_sites + pairs.len());

        let onsite_shift = (-detuning).max(0.0);
        let mut energy_shift = 0.0;

        for site in 0..num_sites {
            terms.push(SseTerm::SiteConstant {
                site: site as u32,
                amplitude: 0.5 * omega,
            });

            terms.push(SseTerm::SpinFlip {
                site: site as u32,
                amplitude: 0.5 * omega,
            });

            terms.push(SseTerm::RydbergDetuning {
                site: site as u32,
                detuning,
                shift: onsite_shift,
            });

            energy_shift += 0.5 * omega + onsite_shift;
        }

        for (site_i, site_j) in pairs {
            let distance = geometry
                .distance(site_i, site_j)
                .map_err(|error| SseModelError::GeometryError(error.to_string()))?;

            if distance <= 0.0 || !distance.is_finite() {
                return Err(SseModelError::InvalidCoefficient {
                    name: "Rydberg pair distance",
                    value: distance,
                });
            }

            let interaction = c6 / distance.powi(6);
            let pair_shift = interaction.max(0.0);

            terms.push(SseTerm::RydbergInteraction {
                site_i,
                site_j,
                interaction,
                shift: pair_shift,
            });

            energy_shift += pair_shift;
        }

        Self::new(num_sites, terms, energy_shift, ModelFamily::Rydberg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoundaryCondition, Geometry};

    #[test]
    fn tfim_decomposition_reconstructs_diagonal_energy() {
        let geometry = Geometry::chain(2, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::tfim(&geometry, &[(0, 1)], 1.25, 0.75).unwrap();

        for state in [
            [Spin::Up, Spin::Up],
            [Spin::Up, Spin::Down],
            [Spin::Down, Spin::Up],
            [Spin::Down, Spin::Down],
        ] {
            let diagonal_sum: f64 = model
                .diagonal_term_indices()
                .iter()
                .map(|&index| model.matrix_element(index as usize, &state).unwrap())
                .sum();
            let physical = -1.25 * state[0].value() as f64 * state[1].value() as f64;

            assert!((model.energy_shift() - diagonal_sum - physical).abs() < 1.0e-12);
        }
    }

    #[test]
    fn transverse_vertices_have_equal_diagonal_and_flip_weights() {
        let geometry = Geometry::chain(1, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::tfim(&geometry, &[], 1.0, 0.75).unwrap();

        assert_eq!(model.num_terms(), 2);
        assert_eq!(model.operator_kind(0).unwrap(), OperatorKind::Diagonal);
        assert_eq!(model.operator_kind(1).unwrap(), OperatorKind::OffDiagonal);
        assert_eq!(model.matrix_element(0, &[Spin::Up]).unwrap(), 0.75);
        assert_eq!(model.matrix_element(1, &[Spin::Up]).unwrap(), 0.75);
    }

    #[test]
    fn rydberg_decomposition_reconstructs_diagonal_energy() {
        let geometry = Geometry::chain(2, BoundaryCondition::Open).unwrap();
        let model = LocalSseModel::rydberg(&geometry, 1.2, 0.4, 2.0).unwrap();

        for state in [
            [Spin::Up, Spin::Up],
            [Spin::Up, Spin::Down],
            [Spin::Down, Spin::Up],
            [Spin::Down, Spin::Down],
        ] {
            let diagonal_sum: f64 = model
                .diagonal_term_indices()
                .iter()
                .map(|&index| model.matrix_element(index as usize, &state).unwrap())
                .sum();
            let n0 = state[0].occupation();
            let n1 = state[1].occupation();
            let physical = -0.4 * (n0 + n1) + 2.0 * n0 * n1;

            assert!((model.energy_shift() - diagonal_sum - physical).abs() < 1.0e-12);
        }
    }
}
