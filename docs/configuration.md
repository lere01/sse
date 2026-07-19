# Configuration reference

The command-line interface accepts the strict `sse-run-v1` YAML schema. Unknown
fields are errors. This catches misspelled physics parameters instead of
silently running a different calculation.

Start from [`configs/tfim-chain.yaml`](https://github.com/lere01/sse/blob/main/configs/tfim-chain.yaml)
or [`configs/rydberg-chain.yaml`](https://github.com/lere01/sse/blob/main/configs/rydberg-chain.yaml),
then run `sse validate` before committing compute time.

## Top-level fields

```yaml
schema_version: sse-run-v1
name: descriptive run name
model: {}
simulation: {}
execution: {}
initial_state: down
```

- `schema_version` must be exactly `sse-run-v1`.
- `name` is copied into the manifest and summary.
- `model` selects the Hamiltonian and geometry.
- `simulation` controls inverse temperature and sweep counts.
- `execution` controls independent chains, workers, and deterministic seeds.
- `initial_state` is `down`, `up`, or `alternating`.

## Geometries

A chain uses:

```yaml
geometry:
  kind: chain
  length: 16
  boundary: periodic
```

A rectangular lattice uses:

```yaml
geometry:
  kind: rectangular
  lx: 8
  ly: 8
  boundary_x: periodic
  boundary_y: periodic
```

Custom open coordinates use:

```yaml
geometry:
  kind: custom
  coordinates:
    - [0.0, 0.0]
    - [1.0, 0.0]
    - [0.5, 0.866025403784]
```

## Models

The TFIM model requires non-negative `j` and `h`:

```yaml
model:
  kind: tfim
  geometry: { kind: chain, length: 16, boundary: periodic }
  j: 1.0
  h: 2.0
```

The Rydberg model permits signed `detuning` and `c6`, while `omega` must be
non-negative:

```yaml
model:
  kind: rydberg
  geometry: { kind: chain, length: 16, boundary: open }
  omega: 1.0
  detuning: 2.0
  c6: 1.0
  update: local
```

`update` is either `local` or `global_reference`. The latter is a validation
reference and may have poor acceptance.

## Simulation schedule

```yaml
simulation:
  beta: 8.0
  operator_string_length: 256
  thermalization_sweeps: 5000
  measurement_sweeps: 50000
  sweeps_per_measurement: 1
```

All fields except `thermalization_sweeps` must be positive. The operator string
grows automatically if it approaches its initial cutoff.

## Execution

```yaml
execution:
  chains: 4
  threads: 4
  seed: 24301
```

The defaults are four chains, one worker, and seed zero. Chain seeds depend only
on the master seed and chain index. Worker scheduling therefore does not change
the individual trajectories within the same software version.
