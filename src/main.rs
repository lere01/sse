//! Minimal construction smoke test for the `sse` crate.
//!
//! The binary builds an 8x8 periodic TFIM decomposition, creates an alternating
//! spin state and identity-filled operator string, verifies trace closure, and
//! prints the resulting model dimensions. For sampled benchmarks, see the
//! `tfim_benchmark`, `rydberg_benchmark`, and `rydberg_scaling` examples.

use sse::{BoundaryCondition, Geometry, LocalSseModel, SSEState, Spin, SseModel};

/// Builds and validates the demonstration model.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let geometry = Geometry::rectangular(
        8,
        8,
        BoundaryCondition::Periodic,
        BoundaryCondition::Periodic,
    )?;

    let nearest_neighbour_pairs = geometry.pairs_at_distance_squared(1.0, 1.0e-12)?;

    let model = LocalSseModel::tfim(
        &geometry,
        &nearest_neighbour_pairs,
        1.0, // J
        0.5, // h
    )?;

    let basis_state = (0..model.num_sites())
        .map(|site| if site % 2 == 0 { Spin::Up } else { Spin::Down })
        .collect();

    let state = SSEState::new(&model, basis_state, 256)?;

    state.validate_trace(&model)?;

    println!("sites: {}", model.num_sites());
    println!("SSE terms: {}", model.num_terms());
    println!("energy shift: {}", model.energy_shift());

    Ok(())
}
