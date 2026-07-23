# Run it in your browser

The [interactive demo](https://lere01.github.io/sse/demo/) runs the same
stochastic series expansion engine as the installed CLI — compiled to
WebAssembly and executed entirely on your device. Nothing is uploaded, and
there is no server: independent Monte Carlo chains run in Web Workers on
your own CPU.

## What it computes

Finite-temperature energy estimates for transverse-field Ising and Rydberg
lattices up to 36 sites, with the same statistical reporting as the CLI:
between-chain standard errors, split \(\hat R\), and autocorrelation-adjusted
effective sample sizes. In the default "β ladder" mode the page samples
three inverse temperatures up to \(\beta = 2L\) and shows the energy
flattening toward the ground state, so the headline number is visibly a
low-temperature estimate — never a claim of exact diagonalization.

## Reproducibility

A browser run is **bit-for-bit identical** to a native run of the same
version: chain seeds derive from the versioned `qslib-seed-v1` scheme, and
the update sequence matches the CLI runner exactly. This is enforced by
tests that replay a CLI-generated fixture through the WebAssembly module
and require identical expansion-order series.

Every configuration on the page mirrors into an `sse-run-v1` YAML document.
To reproduce a browser result natively:

```bash
# paste the YAML from the "Reproduce locally" card into run.yaml
sse run --config run.yaml --output results/run-01
```

The energy per site, standard error, and every chain's series will match
the browser exactly.

## Limits, and what lies beyond them

Browser runs are capped at 36 sites (and 16 sites for Rydberg models, whose
local world-line updates are substantially more expensive per sweep). Past
those limits the page does not refuse — the run button becomes an export:
the same YAML plus a link to the
[release binaries](https://github.com/lere01/sse/releases). Larger lattices,
longer chains, and durable artifacts with restart support are exactly what
the CLI is for.

## Running the demo locally

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.126 --locked
sse-web/scripts/serve_demo.sh          # builds and serves on :8321
```
