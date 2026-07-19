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

Validate the included configuration, then run it:

```bash
cargo run --release -- validate --config configs/tfim-chain.yaml
cargo run --release -- run \
  --config configs/tfim-chain.yaml \
  --output results/tfim-chain
```

Inspect the completed artifact directory:

```bash
cargo run --release -- inspect results/tfim-chain
```

The run reports and stores:

- Total sample count
- Energy per site
- Between-chain standard error
- Per-chain timing and acceptance statistics
- Raw expansion-order series
- Autocorrelation time, effective sample size, and split \(\hat R\)
- Complete configuration and provenance metadata

The example is a starting point, not a convergence certificate. Repeat the run
with larger \(\beta\), longer thermalization, more measurements, and multiple
system sizes before interpreting a physical trend.

## First Rydberg calculation

The Rydberg example configuration uses the production local update:

```bash
cargo run --release -- validate --config configs/rydberg-chain.yaml
cargo run --release -- run \
  --config configs/rydberg-chain.yaml \
  --output results/rydberg-chain
```

Set `model.update` to `global_reference` only for targeted small-system
validation. See the [configuration reference](configuration.md) and
[artifact contract](artifacts.md) before constructing a production campaign.

## Local API documentation

Generate searchable Rust API documentation with:

```bash
cargo doc --no-deps --open
```

The public website also hosts the [Rust API reference](api.md).
