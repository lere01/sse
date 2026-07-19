//! Measures autocorrelation and throughput of Rydberg SSE updates.
//!
//! Invoke with:
//!
//! ```text
//! cargo run --release --example rydberg_scaling -- \
//!     [size] [omega] [detuning] [c6] [beta] [thermalization] \
//!     [measurements] [local|global] [seed]
//! ```
//!
//! The example uses an open square lattice. The local update is the production
//! path; the global corrected update is available as a comparison reference.

use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

use rand::{rngs::StdRng, SeedableRng};
use sse::{BoundaryCondition, Geometry, LocalSseModel, SSEState, Spin, SseModel, SseSampler};

/// Parses arguments, samples energy density, and reports correlation diagnostics.
fn main() -> Result<(), Box<dyn Error>> {
    let size = argument(1, 3_usize)?;
    let omega = argument(2, 1.0_f64)?;
    let detuning = argument(3, 2.0_f64)?;
    let c6 = argument(4, 1.0_f64)?;
    let beta = argument(5, 4.0_f64)?;
    let thermalization = argument(6, 5_000_usize)?;
    let measurements = argument(7, 20_000_usize)?;
    let update = argument(8, String::from("local"))?;
    let seed = argument(9, 24301_u64)?;
    if update != "local" && update != "global" {
        return Err("update must be either 'local' or 'global'".into());
    }

    let geometry = Geometry::square(size, BoundaryCondition::Open)?;
    let model = Arc::new(LocalSseModel::rydberg(&geometry, omega, detuning, c6)?);
    let num_sites = model.num_sites();
    let cutoff = (2.0 * beta * model.energy_shift()).ceil().max(64.0) as usize;
    let state = SSEState::new(&*model, vec![Spin::Down; num_sites], cutoff)?;
    let rng = StdRng::seed_from_u64(seed);
    let mut sampler = SseSampler::with_shared_model(model, state, beta, rng)?;

    for _ in 0..thermalization {
        if update == "local" {
            sampler.rydberg_sweep()?;
        } else {
            sampler.rydberg_global_sweep()?;
        }
    }

    let started = Instant::now();
    let mut energies = Vec::with_capacity(measurements);
    let mut proposed = 0_usize;
    let mut accepted = 0_usize;
    for _ in 0..measurements {
        let (_, stats) = if update == "local" {
            sampler.rydberg_sweep()?
        } else {
            sampler.rydberg_global_sweep()?
        };
        proposed += stats.proposals;
        accepted += stats.proposals_accepted;
        energies.push(sampler.energy_estimator() / num_sites as f64);
    }
    let duration = started.elapsed();
    let diagnostics = diagnostics(&energies);

    println!(
        "{size}x{size} open Rydberg, omega={omega}, detuning={detuning}, c6={c6}, beta={beta}"
    );
    println!("update: {update}");
    println!("thermalization sweeps: {thermalization}");
    println!("measurement sweeps: {measurements}");
    println!("energy per site: {:.12}", diagnostics.mean);
    println!("naive standard error: {:.12}", diagnostics.naive_error);
    println!("autocorrelation time: {:.6}", diagnostics.tau_integrated);
    println!(
        "effective sample size: {:.1}",
        diagnostics.effective_samples
    );
    println!(
        "autocorrelation-adjusted error: {:.12}",
        diagnostics.adjusted_error
    );
    println!(
        "proposal acceptance: {:.6}",
        accepted as f64 / proposed as f64
    );
    println!("measurement time: {:.6} s", duration.as_secs_f64());
    println!(
        "throughput: {:.3} sweeps/s",
        measurements as f64 / duration.as_secs_f64()
    );
    println!(
        "CSV_RESULT,{size}x{size},{update},{:.12},{:.12},{:.6},{:.1},{:.6},{:.3}",
        diagnostics.mean,
        diagnostics.adjusted_error,
        diagnostics.tau_integrated,
        diagnostics.effective_samples,
        accepted as f64 / proposed as f64,
        measurements as f64 / duration.as_secs_f64()
    );
    Ok(())
}

/// Summary statistics for a correlated scalar time series.
struct Diagnostics {
    /// Arithmetic sample mean.
    mean: f64,
    /// Standard error under an independent-sample assumption.
    naive_error: f64,
    /// Integrated autocorrelation time from the initial positive sequence.
    tau_integrated: f64,
    /// Approximate independent-sample-equivalent count.
    effective_samples: f64,
    /// Standard error inflated by the estimated autocorrelation.
    adjusted_error: f64,
}

/// Estimates mean, variance, and integrated autocorrelation diagnostics.
///
/// Correlations are summed until the first non-positive or non-finite value,
/// with a maximum lag of 1000 samples.
fn diagnostics(values: &[f64]) -> Diagnostics {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let variance = values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / (count - 1.0);
    let mut correlation_sum = 0.0;
    let max_lag = values.len().saturating_sub(1).min(1_000);
    for lag in 1..=max_lag {
        let covariance = values[..values.len() - lag]
            .iter()
            .zip(&values[lag..])
            .map(|(left, right)| (left - mean) * (right - mean))
            .sum::<f64>()
            / (values.len() - lag) as f64;
        let correlation = covariance / variance;
        if !correlation.is_finite() || correlation <= 0.0 {
            break;
        }
        correlation_sum += correlation;
    }
    let tau_integrated = 0.5 + correlation_sum;
    let effective_samples = (count / (2.0 * tau_integrated)).max(1.0);
    Diagnostics {
        mean,
        naive_error: (variance / count).sqrt(),
        tau_integrated,
        effective_samples,
        adjusted_error: (variance / effective_samples).sqrt(),
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
