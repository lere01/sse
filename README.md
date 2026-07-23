# SSE

[User guide](https://lere01.github.io/sse/) |
[Rust API](https://lere01.github.io/sse/api/sse/) |
[Source](https://github.com/lere01/sse) |
[Releases](https://github.com/lere01/sse/releases)

`sse` is a Rust implementation of finite-temperature stochastic series
expansion quantum Monte Carlo for spin and Rydberg lattice models.

The crates.io package is named `quantum-sse` because the unrelated `sse`
package name was already allocated. The installed command and Rust library
import remain `sse`.

## Try it in your browser

The [interactive demo](https://lere01.github.io/sse/demo/) runs this exact
engine as WebAssembly on your own device — TFIM and Rydberg lattices up to
36 sites, with live convergence plots and error bars. Browser runs are
bit-for-bit reproducible with the CLI: every result exports the
`sse-run-v1` YAML that replays it natively.

The current source version is **0.2.0**. It provides a configuration-driven
command-line interface for transverse-field Ising and Rydberg simulations.
Runs preserve raw measurements, provenance, statistical diagnostics, and
independently resumable chain artifacts. Since 0.2.0 the Monte Carlo engine is
the published [`qslib-quantum`](https://crates.io/crates/qslib-quantum) SSE
backend; this crate is the thin, physicist-facing binary over it.

## Supported physics

### Transverse-field Ising model

The implemented Hamiltonian is

$$
\begin{equation}
H = -J \sum_{\langle i,j \rangle} \sigma^z_i \sigma^z_j - h \sum_i \sigma^x_i,
\end{equation}
$$

where nearest-neighbour pairs are supplied explicitly or generated from a
geometry. The built-in SSE decomposition requires `J >= 0` and `h >= 0` and
uses non-negative local matrix elements. TFIM sampling combines diagonal
insertion and removal updates with a linked-cluster update.

### Rydberg model

The implemented Hamiltonian is

$$
\begin{equation}
H = -\frac{\Omega}{2} \sum_i \sigma^x_i
    - \Delta \sum_i n_i
    + \sum_{i<j} \frac{C_6}{r_{ij}^6} n_i n_j.
\end{equation}
$$

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

$$
\begin{equation}
H = E_{\mathrm{shift}} - \sum_a B_a,
\end{equation}
$$

with non-negative sampled matrix elements of the local operators `B_a`. If `n`
is the number of non-identity operators in the SSE string, the instantaneous
energy estimator is

$$
\begin{equation}
E = E_{\mathrm{shift}} - \frac{n}{\beta}.
\end{equation}
$$

The heat-capacity estimator, in units with $k_B = 1$, is

$$
\begin{equation}
C = \langle n^2 \rangle - \langle n \rangle^2 - \langle n \rangle.
\end{equation}
$$

Independent-chain execution reports uncertainty from the variation of chain
means. The Rydberg scaling example also demonstrates an integrated
autocorrelation-time estimate. Production studies should verify thermalization,
autocorrelation, operator-string headroom, finite-size effects, and convergence
with respect to inverse temperature.

## Requirements

- Rust 1.85 or newer
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

Build the complete physics-first guide locally with
[mdBook](https://rust-lang.github.io/mdBook/):

```bash
mdbook build
```

The generated guide is written to `target/book`. The public documentation site
combines that guide with the Rust API under `/api/sse/`.

## Quick start

Validate an included physics configuration, execute it, and inspect its durable
result directory:

```bash
cargo run --release -- validate --config configs/tfim-chain.yaml
cargo run --release -- run \
  --config configs/tfim-chain.yaml \
  --output results/tfim-chain
cargo run --release -- inspect results/tfim-chain
```

The strict `sse-run-v1` YAML schema rejects unknown fields and validates the
geometry, Hamiltonian, SSE decomposition, and execution settings before
sampling. See the [configuration reference](https://lere01.github.io/sse/configuration.html)
and [artifact contract](https://lere01.github.io/sse/artifacts.html).

An interrupted run can reuse completed independent chains:

```bash
cargo run --release -- run \
  --config configs/tfim-chain.yaml \
  --output results/tfim-chain \
  --resume
```

Fresh runs refuse existing paths. Intentional replacement requires `--force`.

## Benchmark examples

The Cargo examples remain useful for focused scaling and exact-reference work.
Their positional arguments are optional and fall back to defaults.

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

## Programmatic use

This crate no longer ships a Rust library; the sampling engine lives in the
published [`qslib-quantum`](https://crates.io/crates/qslib-quantum) crate.
Rust programs should depend on it directly:

```toml
[dependencies]
qslib-quantum = { version = "0.2.0", features = ["sse"] }
```

The qslib SSE backend provides the sign-safe TFIM and Rydberg decompositions,
the linked-cluster and world-line update families, deterministic chain seeds,
and recorded measurement series that this binary orchestrates. See the
[qslib SSE guide](https://lere01.github.io/qslib/sse.html) for the library
workflow.

## Reproducibility

Parallel chains derive deterministic 32-byte seeds from the master seed and
chain index under the versioned qslib `qslib-seed-v1` scheme. Changing the
number of Rayon worker threads does not change the individual chain
trajectories within a software version. The CLI records the following in
its resolved configuration, manifest, summary, and chain artifacts:

- Software version or Git revision
- Complete Hamiltonian and geometry parameters
- Inverse temperature
- Initial operator-string cutoff
- Thermalization and measurement sweep counts
- Number of chains and threads
- Master seed
- Update algorithm

## Current limitations

- The `sse-run-v1` configuration format is YAML only.
- Measurement output is JSON and CSV rather than a columnar format.
- Restart granularity is one completed independent chain.
- Built-in physical models are currently limited to TFIM and Rydberg systems.
- Custom periodic simulation cells are not supported.
- The TFIM local decomposition currently accepts only non-negative `J` and `h`.
- The dense exact Rydberg benchmark is limited to tiny validation systems.
- Users must assess equilibration, autocorrelation, and finite-size convergence
  for their physical regime.

## Roadmap

The physics-first guide and automated GitHub Pages build are maintained in this
repository. Planned product work includes:

- Additional physical observables and model families
- A columnar measurement artifact for very large campaigns
- Mid-chain checkpointing where its storage cost is justified
- Prebuilt command-line releases that do not require a Rust installation

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
