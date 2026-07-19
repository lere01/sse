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
//! The lattice is square and periodic with `J = 1`. `measurement_sweeps` is
//! divided across chains. Defaults are 8, 2, 16, 50000, `0x5eed`, at least four
//! chains, and all available hardware threads.

use std::error::Error;
use std::sync::Arc;

use sse::{
    run_parallel_tfim, BoundaryCondition, Geometry, LocalSseModel, ParallelSimulationConfig,
    SimulationConfig, Spin, SseModel,
};

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

    let geometry = Geometry::square(linear_size, BoundaryCondition::Periodic)?;
    let pairs = geometry.pairs_at_distance_squared(1.0, 1.0e-12)?;
    let model = Arc::new(LocalSseModel::tfim(&geometry, &pairs, 1.0, h_over_j)?);
    let initial_cutoff = suggested_cutoff(model.num_sites(), beta, h_over_j);
    let measurements_per_chain = measurement_sweeps.div_ceil(chains);
    let result = run_parallel_tfim(
        Arc::clone(&model),
        &vec![Spin::Up; model.num_sites()],
        ParallelSimulationConfig {
            chains,
            threads,
            master_seed: seed,
            beta,
            operator_string_length: initial_cutoff,
            simulation: SimulationConfig {
                thermalization_sweeps: 5_000,
                measurement_sweeps: measurements_per_chain,
                sweeps_per_measurement: 1,
            },
        },
    )?;

    println!("{linear_size}x{linear_size} open TFIM, J=1, h/J={h_over_j}, beta={beta}");
    println!("chains / Rayon threads: {chains} / {threads}");
    println!("samples: {}", result.combined_energy.samples);
    println!(
        "energy per site: {:.12}",
        result.combined_energy.energy_per_site
    );
    println!(
        "between-chain standard error: {:.12}",
        result.combined_energy.chain_standard_error
    );
    println!("wall time: {:.3} s", result.wall_time.as_secs_f64());
    let chain_seconds: f64 = result
        .chains
        .iter()
        .map(|chain| chain.simulation.timing.total.as_secs_f64())
        .sum();
    println!("summed chain time: {chain_seconds:.3} s");
    if let Some(reference) = reference_energy(linear_size, h_over_j) {
        println!("reference energy per site: {reference:.12}");
        println!(
            "absolute difference: {:.12}",
            (result.combined_energy.energy_per_site - reference).abs()
        );
    }

    Ok(())
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
