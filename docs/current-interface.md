# Command-line interface

The `sse` binary provides three stable workflow commands.

## Validate

Validate YAML, schema fields, geometry, Hamiltonian coefficients, the local SSE
decomposition, and execution settings without starting sampling:

```bash
sse validate --config configs/tfim-chain.yaml
```

Use `--print-resolved` to emit the canonical configuration.

## Run

Execute independent chains and write durable artifacts:

```bash
sse run \
  --config configs/tfim-chain.yaml \
  --output results/tfim-chain
```

A fresh run refuses an existing output path. Use `--resume` to reuse completed
chains from an interrupted run with an identical resolved configuration. Use
`--force` only when intentionally replacing an existing run directory.

The terminal prints the energy density and between-chain standard error. The
JSON and CSV artifacts are the authoritative machine-readable output.

## Inspect

Read status and summary data without modifying a run:

```bash
sse inspect results/tfim-chain
sse inspect results/tfim-chain --json
```

`inspect` works for running, failed, and completed manifests. Aggregate results
are present after all chains finish.

## Exit behavior

Successful commands return zero. Configuration, simulation, and artifact errors
return one. Command-line syntax errors are reported by the argument parser and
return two. Diagnostics are sent to standard error; requested structured output
is sent to standard output.

## Cargo invocation

Before installing a release binary, invoke the same interface through Cargo:

```bash
cargo run --release -- validate --config configs/tfim-chain.yaml
cargo run --release -- run --config configs/tfim-chain.yaml --output results/tfim
```

The benchmark examples remain available for focused performance and exact-
reference work, but new automated workflows should use the versioned CLI.
