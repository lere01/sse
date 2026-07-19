# Getting started

Version 0.1.0 is distributed as source and requires a stable Rust toolchain.
Install Rust with [rustup](https://rustup.rs/), clone the repository, and verify
the complete crate before running a calculation:

```bash
git clone https://github.com/lere01/sse.git
cd sse
cargo test --all-targets
cargo test --doc
```

## First TFIM calculation

Run four independent chains on a periodic \(4 \times 4\) lattice:

```bash
cargo run --release --example tfim_benchmark -- \
  4 2.0 8.0 10000 24301 4 4
```

The positional arguments are:

```text
linear_size h_over_j beta measurement_sweeps seed chains threads
```

The example fixes \(J=1\), performs 5,000 thermalization sweeps per chain,
and divides the requested measurement count across the chains. It reports:

- Total sample count
- Energy per site
- Between-chain standard error
- Wall time and summed chain time
- A reference comparison when one is available for the parameter point

The example is a starting point, not a convergence certificate. Repeat the run
with larger \(\beta\), longer thermalization, more measurements, and multiple
system sizes before interpreting a physical trend.

## First Rydberg calculation

The scaling example exposes both Rydberg update implementations:

```bash
cargo run --release --example rydberg_scaling -- \
  3 1.0 2.0 1.0 4.0 5000 20000 local 24301
```

Its arguments are:

```text
size omega detuning c6 beta thermalization measurements update seed
```

Use `local` for the production world-line update. Use `global` primarily as a
small-system correctness reference. The program reports an integrated
autocorrelation estimate, effective sample size, adjusted uncertainty,
proposal acceptance, and throughput.

## Local API documentation

Generate searchable Rust API documentation with:

```bash
cargo doc --no-deps --open
```

The public website also hosts the [Rust API reference](api.md).
