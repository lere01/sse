# Run artifacts and restart

`sse run` writes an immutable, versioned directory rather than relying on
terminal output. A completed directory contains:

```text
run/
├── config.input.yaml
├── config.resolved.yaml
├── manifest.json
├── checkpoint.json
├── measurements.csv
├── summary.json
└── chains/
    ├── chain-000000.json
    └── ...
```

`config.input.yaml` preserves the submitted text. `config.resolved.yaml` is the
canonical validated configuration and is the authority for resume checks.

`manifest.json` records the `sse-artifacts-v2` schema, software version, optional
embedded Git revision, timestamps, attempt count, lifecycle status, and chain
completion count.

Each chain JSON is written atomically after that chain completes. It contains
the deterministic seed, thermodynamics, update acceptance counts, timing,
autocorrelation diagnostics, and raw expansion-order series. These files are
the restart boundary.

`measurements.csv` combines raw series using the stable columns:

```text
chain_index,measurement_index,expansion_order
```

`summary.json` reports the mean of independent-chain energy densities,
between-chain standard error, split \(\hat R\), minimum effective sample size,
and explicit diagnostic warnings. Between-chain standard error is `null` when
only one chain is present because it is not statistically defined.

## Restart behavior

Resume an interrupted directory with:

```bash
sse run --config run.yaml --output results/run-01 --resume
```

The supplied configuration must exactly match `config.resolved.yaml`. Completed
chains are reused. An interrupted active chain restarts from its deterministic
seed, so no partial or ambiguous state is accepted.

Fresh runs refuse existing paths. Replacing a path requires the explicit
`--force` flag. Safety checks reject the current working directory, file-system
root, and non-empty directories that do not contain a recognized SSE manifest.

Artifact schemas are versioned independently of configuration schemas. Analysis
programs should check `artifact_schema_version` rather than assuming every
future release has the same fields.
