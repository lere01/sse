# SSE

[User guide](https://lere01.github.io/sse/) |
[Rust API](https://lere01.github.io/sse/api/sse/) |
[Source](https://github.com/lere01/sse)

`sse` is a Rust implementation of finite-temperature stochastic series
expansion quantum Monte Carlo for spin and Rydberg lattice models.

The current release is **version 0.1.0**. It provides a tested Rust library and
command-line examples for transverse-field Ising and Rydberg simulations. A
configuration-driven, physicist-facing command-line interface is planned but is
not part of this release.

## Supported physics

### Transverse-field Ising model

The implemented Hamiltonian is

```text
H = -J sum_<ij> sigma_z(i) sigma_z(j) - h sum_i sigma_x(i),
```

where nearest-neighbour pairs are supplied explicitly or generated from a
geometry. The built-in SSE decomposition requires `J >= 0` and `h >= 0` and
uses non-negative local matrix elements. TFIM sampling combines diagonal
insertion and removal updates with a linked-cluster update.

### Rydberg model

The implemented Hamiltonian is

```text
H = -(omega / 2) sum_i sigma_x(i)
    - detuning sum_i n(i)
    + sum_(i<j) [c6 / r(i,j)^6] n(i) n(j).
```

The occupation convention is

```text
Spin::Down -> n = 0
Spin::Up   -> n = 1.
```

The Rabi frequency must satisfy `omega >= 0`. Detuning and `c6` may have either
sign. Every unordered pair of sites is included in the long-range interaction.
The production sampler uses local world-line Metropolis moves. A global
Metropolis-corrected cluster update is retained as a validation reference.

## Geometries

The library supports:

- Open or periodic one-dimensional chains
- Rectangular and square lattices with independently selected boundary
  conditions along each axis
- Open custom two-dimensional coordinates

Chains and rectangular lattices have unit spacing. Periodic distances use the
minimum-image convention. Rectangular sites use row-major indexing:

```text
site = x + lx * y.
```

## Thermodynamic estimators

The local decomposition represents the Hamiltonian as

```text
H = energy_shift - sum_a B_a,
```

with non-negative sampled matrix elements of the local operators `B_a`. If `n`
is the number of non-identity operators in the SSE string, the instantaneous
energy estimator is

```text
E = energy_shift - n / beta.
```

The heat-capacity estimator, in units with `k_B = 1`, is

```text
C = <n^2> - <n>^2 - <n>.
```

Independent-chain execution reports uncertainty from the variation of chain
means. The Rydberg scaling example also demonstrates an integrated
autocorrelation-time estimate. Production studies should verify thermalization,
autocorrelation, operator-string headroom, finite-size effects, and convergence
with respect to inverse temperature.

## Requirements

- A recent stable Rust toolchain
- Cargo

Install Rust through [rustup](https://rustup.rs/) if it is not already
available.

## Build and test

From the crate directory:

```bash
cargo build --release
cargo test --all-targets
cargo test --doc
```

Generate the Rust API documentation with:

```bash
cargo doc --no-deps --open
```

## Quick start

The current release exposes simulations through examples. Positional arguments
are optional and fall back to defaults.

### Parallel TFIM simulation

```bash
cargo run --release --example tfim_benchmark -- \
  4 2.0 8.0 10000 24301 4 4
```

Arguments are:

```text
linear_size h_over_j beta measurement_sweeps seed chains threads
```

This constructs a periodic square lattice with `J = 1`, divides measurements
across independent chains, and reports energy density, between-chain standard
error, and timing information.

### Rydberg update scaling

```bash
cargo run --release --example rydberg_scaling -- \
  3 1.0 2.0 1.0 4.0 5000 20000 local 24301
```

Arguments are:

```text
size omega detuning c6 beta thermalization measurements update seed
```

The `update` argument is either `local` or `global`. This example reports energy
density, naive and autocorrelation-adjusted errors, effective sample size,
proposal acceptance, and throughput.

### Exact Rydberg validation

```bash
cargo run --release --example rydberg_benchmark -- \
  2 1.0 1.0 1.0 4.0 20000 8 24301 local
```

Arguments are:

```text
linear_size omega detuning c6 beta measurement_sweeps chains seed update
```

The example constructs the dense Hamiltonian, diagonalizes it with a small
Jacobi solver, and checks the SSE thermal energy against the exact result. Exact
validation is intentionally limited to at most six sites.

## Library example

```rust
use rand::{rngs::StdRng, SeedableRng};
use sse::{
    BoundaryCondition, Geometry, LocalSseModel, SSEState, SimulationConfig,
    Spin, SseModel, SseSampler,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let geometry = Geometry::chain(4, BoundaryCondition::Periodic)?;
    let pairs = geometry.pairs_at_distance_squared(1.0, 1.0e-12)?;
    let model = LocalSseModel::tfim(&geometry, &pairs, 1.0, 0.5)?;
    let state = SSEState::new(&model, vec![Spin::Up; model.num_sites()], 64)?;
    let rng = StdRng::seed_from_u64(7);
    let mut sampler = SseSampler::new(model, state, 2.0, rng)?;

    let results = sampler.run_tfim(SimulationConfig {
        thermalization_sweeps: 1_000,
        measurement_sweeps: 10_000,
        sweeps_per_measurement: 1,
    })?;

    println!(
        "energy per site = {}",
        results.thermodynamics.energy_per_site
    );
    Ok(())
}
```

## Reproducibility

Parallel chains derive deterministic seeds from a master seed and chain index.
Changing the number of Rayon worker threads does not change the individual
chain trajectories. Record at least the following for a reproducible study:

- Software version or Git revision
- Complete Hamiltonian and geometry parameters
- Inverse temperature
- Initial operator-string cutoff
- Thermalization and measurement sweep counts
- Number of chains and threads
- Master seed
- Update algorithm

## Current limitations

- There is not yet a stable configuration-file CLI.
- Measurement-series and result-artifact schemas are not yet implemented.
- Built-in physical models are currently limited to TFIM and Rydberg systems.
- Custom periodic simulation cells are not supported.
- The TFIM local decomposition currently accepts only non-negative `J` and `h`.
- The dense exact Rydberg benchmark is limited to tiny validation systems.
- Users must assess equilibration, autocorrelation, and finite-size convergence
  for their physical regime.

## Documentation roadmap

Planned work includes:

- A physics-first user guide with detailed conventions and convergence advice
- A versioned YAML configuration schema
- `run`, `validate`, and `inspect` CLI commands
- Versioned JSON and CSV result artifacts
- GitHub Pages documentation with Rust API reference under `/api`
- Prebuilt command-line releases that do not require a Rust installation

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
