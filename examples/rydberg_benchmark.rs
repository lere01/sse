//! Validates the Rydberg SSE energy against exact diagonalization.
//!
//! Invoke with:
//!
//! ```text
//! cargo run --release --example rydberg_benchmark -- \
//!     [linear_size] [omega] [detuning] [c6] [beta] \
//!     [measurement_sweeps] [chains] [seed] [local|global]
//! ```
//!
//! The exact dense Jacobi eigensolver intentionally limits the open square
//! lattice to six sites. The command fails if the SSE mean differs from the
//! exact thermal energy by more than three between-chain standard errors.

use std::error::Error;
use std::sync::Arc;

use rand::{rngs::StdRng, SeedableRng};
use sse::{
    BoundaryCondition, Geometry, LocalSseModel, SSEState, SimulationConfig, Spin, SseModel,
    SseSampler,
};

/// Largest site count accepted by the dependency-free dense eigensolver.
const MAX_EXACT_SITES: usize = 6;

/// Runs exact diagonalization and independent SSE chains for one parameter set.
fn main() -> Result<(), Box<dyn Error>> {
    let linear_size = argument(1, 2_usize)?;
    let omega = argument(2, 1.0_f64)?;
    let detuning = argument(3, 1.0_f64)?;
    let c6 = argument(4, 1.0_f64)?;
    let beta = argument(5, 4.0_f64)?;
    let measurement_sweeps = argument(6, 20_000_usize)?;
    let chains = argument(7, 8_usize)?;
    let seed = argument(8, 0x5eed_u64)?;
    let update = argument(9, String::from("local"))?;
    if update != "local" && update != "global" {
        return Err("update must be either 'local' or 'global'".into());
    }

    validate_finite("omega", omega)?;
    validate_finite("detuning", detuning)?;
    validate_finite("c6", c6)?;
    if omega < 0.0 {
        return Err("omega must be non-negative".into());
    }
    if !beta.is_finite() || beta <= 0.0 {
        return Err("beta must be finite and positive".into());
    }
    if measurement_sweeps == 0 || chains < 2 {
        return Err("measurement sweeps must be positive and chains must be at least two".into());
    }

    let geometry = Geometry::square(linear_size, BoundaryCondition::Open)?;
    let num_sites = geometry.num_sites();
    if num_sites > MAX_EXACT_SITES {
        return Err(format!(
            "exact diagonalization is limited to {MAX_EXACT_SITES} sites; got {num_sites}"
        )
        .into());
    }

    let model = Arc::new(LocalSseModel::rydberg(&geometry, omega, detuning, c6)?);
    let hamiltonian = dense_rydberg_hamiltonian(&geometry, omega, detuning, c6)?;
    let eigenvalues = symmetric_eigenvalues(hamiltonian);
    let thermal = thermal_results(&eigenvalues, beta);

    println!(
        "{linear_size}x{linear_size} open Rydberg, omega={omega}, detuning={detuning}, c6={c6}, beta={beta}"
    );
    println!("sites: {num_sites}");
    println!("Hilbert-space dimension: {}", eigenvalues.len());
    println!("SSE terms: {}", model.num_terms());
    println!("SSE energy shift: {:.12}", model.energy_shift());
    println!("exact ground-state energy: {:.12}", eigenvalues[0]);
    println!(
        "exact ground-state energy per site: {:.12}",
        eigenvalues[0] / num_sites as f64
    );
    println!("exact thermal energy: {:.12}", thermal.energy);
    println!(
        "exact thermal energy per site: {:.12}",
        thermal.energy / num_sites as f64
    );
    println!("exact heat capacity: {:.12}", thermal.heat_capacity);

    let cutoff = (beta * model.energy_shift() * 2.0).ceil().max(64.0) as usize;
    let mut chain_energies = Vec::with_capacity(chains);
    let mut proposals = 0_usize;
    let mut accepted = 0_usize;
    for chain in 0..chains {
        let chain_seed = splitmix64(seed.wrapping_add(chain as u64));
        let state = SSEState::new(&*model, vec![Spin::Down; num_sites], cutoff)?;
        let rng = StdRng::seed_from_u64(chain_seed);
        let mut sampler = SseSampler::with_shared_model(Arc::clone(&model), state, beta, rng)?;
        let config = SimulationConfig {
            thermalization_sweeps: 5_000,
            measurement_sweeps,
            sweeps_per_measurement: 1,
        };
        let result = if update == "local" {
            sampler.run_rydberg(config)?
        } else {
            sampler.run_rydberg_global_reference(config)?
        };
        chain_energies.push(result.thermodynamics.energy);
        proposals += result.clusters.proposals;
        accepted += result.clusters.proposals_accepted;
    }
    let sampled = mean_and_standard_error(&chain_energies);
    let difference = (sampled.0 - thermal.energy).abs();
    let z_score = difference / sampled.1;
    println!("SSE chains: {chains}");
    println!("SSE update: {update}");
    println!("SSE samples: {}", measurement_sweeps * chains);
    println!("SSE thermal energy: {:.12}", sampled.0);
    println!("SSE between-chain standard error: {:.12}", sampled.1);
    println!("SSE absolute difference: {:.12}", difference);
    println!("SSE difference / standard error: {:.6}", z_score);
    println!(
        "Rydberg cluster proposal acceptance: {:.6}",
        accepted as f64 / proposals as f64
    );
    if !z_score.is_finite() || z_score > 3.0 {
        return Err(format!(
            "SSE energy failed the exact benchmark: difference is {z_score:.3} standard errors"
        )
        .into());
    }
    println!("SSE exact-energy check: PASS");
    println!(
        "CSV_RESULT,{linear_size}x{linear_size},{omega},{detuning},{c6},{beta},{:.12},{:.12},{:.6}",
        sampled.0 / num_sites as f64,
        sampled.1 / num_sites as f64,
        accepted as f64 / proposals as f64
    );

    Ok(())
}

/// Returns the arithmetic mean and between-value standard error.
///
/// The caller supplies at least two independent chain estimates.
fn mean_and_standard_error(values: &[f64]) -> (f64, f64) {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let variance = values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / (count - 1.0);
    (mean, (variance / count).sqrt())
}

/// Applies the SplitMix64 output permutation to derive a chain seed.
fn splitmix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Constructs the dense Rydberg Hamiltonian in the occupation basis.
///
/// Basis index bit `i` is the occupation of geometry site `i`. The returned
/// matrix includes every `c6 / r^6` pair interaction and `omega / 2` spin flip.
fn dense_rydberg_hamiltonian(
    geometry: &Geometry,
    omega: f64,
    detuning: f64,
    c6: f64,
) -> Result<Vec<Vec<f64>>, Box<dyn Error>> {
    let num_sites = geometry.num_sites();
    let dimension = 1_usize
        .checked_shl(num_sites as u32)
        .ok_or("Hilbert-space dimension overflow")?;
    let pairs = geometry.all_pairs();
    let mut interactions = Vec::with_capacity(pairs.len());
    for (site_i, site_j) in pairs {
        let distance = geometry.distance(site_i, site_j)?;
        interactions.push((site_i as usize, site_j as usize, c6 / distance.powi(6)));
    }

    let mut matrix = vec![vec![0.0; dimension]; dimension];
    for (basis, matrix_row) in matrix.iter_mut().enumerate() {
        let particles = basis.count_ones() as f64;
        let mut diagonal = -detuning * particles;
        for &(site_i, site_j, interaction) in &interactions {
            let occupied_i = (basis >> site_i) & 1;
            let occupied_j = (basis >> site_j) & 1;
            diagonal += interaction * (occupied_i * occupied_j) as f64;
        }
        matrix_row[basis] = diagonal;

        for site in 0..num_sites {
            let flipped = basis ^ (1 << site);
            matrix_row[flipped] = 0.5 * omega;
        }
    }
    Ok(matrix)
}

/// Computes eigenvalues of a real symmetric matrix using Jacobi rotations.
///
/// This dependency-free implementation is intended only for the tiny exact
/// validation systems accepted by [`MAX_EXACT_SITES`].
fn symmetric_eigenvalues(mut matrix: Vec<Vec<f64>>) -> Vec<f64> {
    let dimension = matrix.len();
    let tolerance = 1.0e-13;
    let max_iterations = 100 * dimension * dimension;

    for _ in 0..max_iterations {
        let mut row = 0;
        let mut column = 0;
        let mut largest = 0.0_f64;
        for (i, matrix_row) in matrix.iter().enumerate() {
            for (j, &value) in matrix_row.iter().enumerate().skip(i + 1) {
                let candidate = value.abs();
                if candidate > largest {
                    largest = candidate;
                    row = i;
                    column = j;
                }
            }
        }
        if largest < tolerance {
            break;
        }

        let app = matrix[row][row];
        let aqq = matrix[column][column];
        let apq = matrix[row][column];
        let angle = 0.5 * (2.0 * apq).atan2(aqq - app);
        let cosine = angle.cos();
        let sine = angle.sin();

        for index in (0..dimension).filter(|&index| index != row && index != column) {
            let aip = matrix[index][row];
            let aiq = matrix[index][column];
            let rotated_p = cosine * aip - sine * aiq;
            let rotated_q = sine * aip + cosine * aiq;
            matrix[index][row] = rotated_p;
            matrix[row][index] = rotated_p;
            matrix[index][column] = rotated_q;
            matrix[column][index] = rotated_q;
        }
        matrix[row][row] = cosine * cosine * app - 2.0 * sine * cosine * apq + sine * sine * aqq;
        matrix[column][column] =
            sine * sine * app + 2.0 * sine * cosine * apq + cosine * cosine * aqq;
        matrix[row][column] = 0.0;
        matrix[column][row] = 0.0;
    }

    let mut eigenvalues: Vec<_> = matrix
        .iter()
        .enumerate()
        .map(|(index, matrix_row)| matrix_row[index])
        .collect();
    eigenvalues.sort_by(f64::total_cmp);
    eigenvalues
}

/// Exact canonical energy and heat capacity at one inverse temperature.
struct ThermalResults {
    /// Canonical mean energy.
    energy: f64,
    /// Canonical heat capacity in units with `k_B = 1`.
    heat_capacity: f64,
}

/// Evaluates stable canonical moments from a sorted eigenvalue spectrum.
///
/// Subtracting the ground-state energy in the Boltzmann exponent avoids
/// overflow without changing normalized expectation values.
fn thermal_results(eigenvalues: &[f64], beta: f64) -> ThermalResults {
    let ground = eigenvalues[0];
    let mut partition = 0.0;
    let mut energy_sum = 0.0;
    let mut energy_squared_sum = 0.0;
    for &energy in eigenvalues {
        let weight = (-beta * (energy - ground)).exp();
        partition += weight;
        energy_sum += weight * energy;
        energy_squared_sum += weight * energy * energy;
    }
    let energy = energy_sum / partition;
    let energy_squared = energy_squared_sum / partition;
    ThermalResults {
        energy,
        heat_capacity: beta * beta * (energy_squared - energy * energy),
    }
}

/// Rejects NaN and infinite command-line coefficients.
fn validate_finite(name: &str, value: f64) -> Result<(), Box<dyn Error>> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{name} must be finite").into())
    }
}

/// Parses one positional argument, falling back to `default` when omitted.
fn argument<T>(position: usize, default: T) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
    T::Err: Error + 'static,
{
    match std::env::args().nth(position) {
        Some(value) => Ok(value.parse()?),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_site_matches_analytic_eigenvalues() {
        let geometry = Geometry::chain(1, BoundaryCondition::Open).unwrap();
        let omega = 1.2;
        let detuning = 0.7;
        let eigenvalues = symmetric_eigenvalues(
            dense_rydberg_hamiltonian(&geometry, omega, detuning, 0.0).unwrap(),
        );
        let center = -0.5 * detuning;
        let radius = 0.5 * (detuning * detuning + omega * omega).sqrt();
        assert!((eigenvalues[0] - (center - radius)).abs() < 1.0e-11);
        assert!((eigenvalues[1] - (center + radius)).abs() < 1.0e-11);
    }
}
