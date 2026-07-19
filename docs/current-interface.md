# Current command-line examples

Version 0.1.0 does not yet provide the planned configuration-driven `sse run`
interface. Its executable simulations are Cargo examples.

## TFIM benchmark

```text
cargo run --release --example tfim_benchmark --
    [linear_size] [h_over_j] [beta] [measurement_sweeps]
    [seed] [chains] [threads]
```

This example uses a periodic square lattice and \(J=1\). It performs fixed
thermalization in every chain, distributes measurements across chains, and
combines their energy-density means.

## Rydberg scaling

```text
cargo run --release --example rydberg_scaling --
    [size] [omega] [detuning] [c6] [beta] [thermalization]
    [measurements] [local|global] [seed]
```

This example uses an open square lattice and records every measurement-phase
energy estimate in memory to calculate a simple autocorrelation diagnostic.

## Exact Rydberg benchmark

```text
cargo run --release --example rydberg_benchmark --
    [linear_size] [omega] [detuning] [c6] [beta]
    [measurement_sweeps] [chains] [seed] [local|global]
```

The example compares the SSE thermal energy with dense exact diagonalization
and fails if their difference exceeds three estimated between-chain standard
errors. Dense validation is limited to six sites.

## Planned interface

A future release will add versioned YAML configuration together with `run`,
`validate`, and `inspect` commands. Until it is implemented and released, do
not rely on those command names in automated workflows.
