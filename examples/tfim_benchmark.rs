//! Runs independent TFIM SSE chains and compares selected cases to references.
//!
//! Invoke with:
//!
//! ```text
//! cargo run --release --example tfim_benchmark -- \
//!     [linear_size] [h_over_j] [beta] [measurement_sweeps] \
//!     [seed] [chains] [threads]
//! ```
//!
//! The lattice is square and open with `J = 1`, matching the stored
//! reference energies. `measurement_sweeps` is
//! divided across chains. Defaults are 8, 2, 16, 50000, `0x5eed`, at least four
//! chains, and all available hardware threads. Sampling uses the qslib TFIM
//! linked-cluster update scheme.

use std::error::Error;
use std::time::Instant;

use qslib::sse::{
    run_parallel_chains_with, BasisSseState, LocalSseModel, Operator, SimulationConfig,
    UpdateScheme,
};
use qslib::{BasisBit, Boundary, RectangularGeometry, ShellTolerance};

/// Parses arguments, runs the parallel benchmark, and prints energy diagnostics.
fn main() -> Result<(), Box<dyn Error>> {
    let linear_size = argument(1, 8_usize)?;
    let h_over_j = argument(2, 2.0)?;
    let beta = argument(3, 16.0)?;
    let measurement_sweeps: usize = argument(4, 50_000)?;
    let seed = argument(5, 0x5eed_u64)?;
    let default_threads = std::thread::available_parallelism()?.get();
    let chains: usize = argument(6, default_threads.max(4))?;
    let threads: usize = argument(7, default_threads)?;

    let geometry =
        RectangularGeometry::new(linear_size, linear_size, Boundary::Open, Boundary::Open)?;
    let num_sites = geometry.site_count().get();
    let pairs: Vec<(u32, u32)> = geometry
        .pairs_at_squared_distance(1.0, ShellTolerance::Absolute(1.0e-12))?
        .into_iter()
        .map(|bond| (bond.first().get(), bond.second().get()))
        .collect();
    let model = LocalSseModel::tfim(num_sites, &pairs, 1.0, h_over_j)?;
    let initial_cutoff = suggested_cutoff(num_sites, beta, h_over_j);
    // Legacy all-up TFIM start state: up is +Z, canonical bit zero.
    let state = BasisSseState::new(
        vec![BasisBit::Zero; num_sites],
        vec![Operator::identity(); initial_cutoff],
    )?;
    let measurements_per_chain = measurement_sweeps.div_ceil(chains);
    let started = Instant::now();
    let results = run_parallel_chains_with(
        model,
        state,
        beta,
        SimulationConfig {
            thermalization_sweeps: 5_000,
            measurement_sweeps: measurements_per_chain,
            sweeps_per_measurement: 1,
        },
        UpdateScheme::TfimCluster,
        seed,
        chains,
        threads,
    )?;
    let wall_time = started.elapsed();

    let samples: u64 = results
        .iter()
        .map(|result| result.thermodynamics.samples)
        .sum();
    let energies: Vec<f64> = results
        .iter()
        .map(|result| result.thermodynamics.energy_per_site)
        .collect();
    let (energy_per_site, standard_error) = mean_and_standard_error(&energies);

    println!("{linear_size}x{linear_size} open TFIM, J=1, h/J={h_over_j}, beta={beta}");
    println!("chains / worker threads: {chains} / {threads}");
    println!("samples: {samples}");
    println!("energy per site: {energy_per_site:.12}");
    println!("between-chain standard error: {standard_error:.12}");
    println!("wall time: {:.3} s", wall_time.as_secs_f64());
    if let Some(reference) = reference_energy(linear_size, h_over_j) {
        println!("reference energy per site: {reference:.12}");
        println!(
            "absolute difference: {:.12}",
            (energy_per_site - reference).abs()
        );
    }

    Ok(())
}

/// Returns the arithmetic mean and between-value standard error.
fn mean_and_standard_error(values: &[f64]) -> (f64, f64) {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    if values.len() < 2 {
        return (mean, 0.0);
    }
    let variance = values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / (count - 1.0);
    (mean, (variance / count).sqrt())
}

/// Estimates a conservative initial operator-string cutoff.
///
/// The sampler can grow this cutoff automatically if the realized expansion
/// order leaves insufficient identity headroom.
fn suggested_cutoff(num_sites: usize, beta: f64, h_over_j: f64) -> usize {
    // Conservative initial headroom for an open square TFIM with J=1.
    (beta * num_sites as f64 * (4.0 + 2.0 * h_over_j))
        .ceil()
        .max(256.0) as usize
}

/// Returns a stored reference energy density for a matching benchmark point.
fn reference_energy(linear_size: usize, h_over_j: f64) -> Option<f64> {
    const REFERENCES: &[(usize, f64, f64)] = &[
        (4, 0.5, -1.544_380_372_203_914_7),
        (4, 1.0, -1.678_778_948_011_559_6),
        (4, 2.0, -2.244_203_601_265_758_7),
        (4, 3.044, -3.178_276_346_678_86),
        (4, 5.0, -5.077_174_926_174_71),
        (8, 0.5, -1.787_184_929_443_562_2),
        (8, 1.0, -1.899_630_184_627_695_5),
        (8, 2.0, -2.361_804_930_420_907_2),
        (8, 3.044, -3.205_040_531_187_53),
        (8, 5.0, -5.090_609_885_090_71),
    ];

    REFERENCES
        .iter()
        .find(|&&(size, ratio, _)| size == linear_size && (ratio - h_over_j).abs() < 1.0e-12)
        .map(|&(_, _, energy)| energy)
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
