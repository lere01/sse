# sse-web: in-browser ground-state estimates over the qslib SSE engine

Status: implemented locally (M1-M4); pending owner review and public launch. Owner: Faith Oyedemi. Target: sse 0.3.0.

## 1. Product definition

A static page on the existing GitHub Pages site where a visitor configures a
TFIM or Rydberg lattice up to 6x6, runs real stochastic series expansion
Monte Carlo **in their own browser**, and reads a low-temperature energy
estimate with honest statistical quality indicators. Beyond the online size
limit, the same page hands the visitor a ready-to-run configuration for the
installed `sse` executable.

### Goals

- Real computation, not a lookup: the browser runs the same qslib kernels,
  same seeds, same streams as the native binary. A browser run at version X
  is bit-identical to `sse run` at version X with the same configuration.
- Zero operational surface: static hosting only, no server, no queue, no
  abuse handling. The visitor's CPU pays for the visitor's request.
- A funnel, not a silo: every result exports the exact `sse-run-v1` YAML and
  CLI invocation that reproduces it natively. Sizes beyond the online limit
  are presented as an invitation to install, never as an error.

### Non-goals

- Exact diagonalization (2^36 states is not a thing; the page never implies
  it). The product is a finite-temperature estimate at large beta with error
  bars, labeled as such.
- Observables beyond energy and heat capacity (the SSE backend measures
  expansion-order thermodynamics only).
- A general qslib playground. If that ever happens it belongs in the qslib
  repository, not here.

## 2. Architecture

```text
GitHub Pages (static)
  |
  |-- index page (docs mdBook, unchanged)
  |-- demo/                         <- new interactive page
  |     index.html  app.js  ui/    (vanilla ES modules, no framework)
  |     sse_web_bg.wasm  sse_web.js (wasm-bindgen output)
  |
Visitor browser
  main thread: form state, lattice preview, plots, aggregation
  worker pool: N Web Workers, one independent chain each
      worker.js -> instantiates wasm -> run_chain_chunk(...) loop
```

Design decisions and their reasons:

- **Chain-level parallelism through Web Workers, no wasm threads.** Shared-
  memory wasm needs SharedArrayBuffer and COOP/COEP headers GitHub Pages
  cannot set. Independent chains need no shared memory: each worker holds
  its own wasm instance, seeds `derive_chain_seed(master, chain)`, and posts
  results back. This is `run_parallel_chains` rebuilt in ~50 lines of JS.
- **Chunked execution.** Workers run sweeps in batches (target ~250 ms per
  batch) and post partial expansion-order series after each batch. This
  gives live-updating estimates, real progress bars, cancellation, and a
  responsive tab. The wasm API is therefore resumable-by-construction:
  `create_chain(...) -> handle`, `advance(handle, sweeps) -> partial`.
- **No framework, no build step beyond wasm-bindgen.** Vanilla ES modules,
  hand-rolled SVG for the lattice preview and plots. The page must be
  self-contained, cacheable, and maintainable by one person. Budget:
  wasm <= 400 KB gzipped, JS + CSS <= 50 KB, interactive < 1 s.
- **Deterministic by contract.** No `Date.now`-derived seeds. The master
  seed is explicit in the form (random button fills one in visibly). Same
  version + same config + same seed = same numbers, browser or native.

## 3. Repository structure

Convert the root manifest to a workspace (the qslib repository uses exactly
this layout; crates.io publishing of the root package is unaffected):

```text
sse/
  Cargo.toml            [package] quantum-sse (root, publishes)
                        [workspace] members = [".", "sse-web"]
  src/                  the CLI binary (unchanged)
  sse-web/
    Cargo.toml          publish = false, crate-type = ["cdylib"]
    src/lib.rs          wasm-bindgen surface over qslib-quantum {sse}
    web/                index.html, app.js, worker.js, ui/*.js, style.css
  docs/                 mdBook (gains a "Run it in your browser" chapter)
  .github/workflows/pages.yml   builds mdBook + wasm, assembles target/site
```

The single workspace `Cargo.lock` is the reproducibility guarantee: the CLI
and the wasm module cannot resolve different qslib versions.

## 4. The wasm API contract

Small, resumable, JSON at the boundary (serde-wasm-bindgen or plain JSON
strings; no custom binary protocol).

```rust
#[wasm_bindgen]
pub struct ChainHandle { /* sampler + scheme + counters, opaque to JS */ }

#[wasm_bindgen]
pub fn create_chain(config_json: &str, chain_index: u32)
    -> Result<ChainHandle, JsError>;
// config: { model, geometry, params, beta, cutoff, master_seed }
// validates the same invariants as the CLI; enforces num_sites <= 36

#[wasm_bindgen]
pub fn advance(handle: &mut ChainHandle, sweeps: u32) -> String;
// runs `sweeps` (thermalization first, then measurement), returns
// { phase, completed, expansion_orders: [...], stats: {...} }

#[wasm_bindgen]
pub fn config_yaml(config_json: &str) -> Result<String, JsError>;
// emits the equivalent sse-run-v1 YAML for the "reproduce locally" card
```

Aggregation (chain means, between-chain SE, split R-hat, ESS) runs in the
main thread in JS; the formulas are ten lines each and keeping them out of
wasm avoids marshalling full series across the boundary more than once.
`std::time` is never used inside the crate (it panics on this target);
timing lives in JS.

## 5. Performance envelope and guardrails

| Case | Verdict | Guardrail |
| --- | --- | --- |
| TFIM up to 6x6, beta ~ 2L | Seconds to ~1 min/chain in wasm; cluster updates keep autocorrelation ~1 | Online default preset |
| Rydberg up to 4x4 | Acceptable (local sweep is O(N * M) per sweep; M grows with the all-pairs energy shift) | Online, with sweep budget |
| Rydberg 5x5-6x6 | Minutes to tens of minutes in a tab | Offered as "run locally" handoff, with a "run anyway" escape hatch and honest ETA |
| Anything > 36 sites | Out of scope online | Size picker continues past 6 but flips the CTA to install instructions |

Hard caps enforced in both the UI and `create_chain`: 36 sites, 16 chains,
5e5 total sweeps per chain, cutoff growth bounded by available memory.
Battery courtesy: workers pause when the tab is hidden (Page Visibility
API), with a visible "paused" state.

## 6. UI/UX design

### Principles

1. **The lattice is the hero.** The visitor configures physics by seeing
   it: a live SVG of the actual sites and bonds, with periodic boundaries
   drawn as wrap arcs. Changing L, boundaries, or model redraws instantly.
2. **Watching convergence is the product.** Monte Carlo's most compelling
   moment is the estimate settling as error bars shrink. The running state
   is a live energy trace per chain converging onto a band - not a spinner.
3. **Uncertainty is a first-class citizen.** Every number ships with its
   error bar; quality is summarized by badges (R-hat, min ESS) with plain-
   language tooltips. No naked point estimates anywhere.
4. **The boundary is an invitation.** Past 6x6 nothing errors; the run
   button becomes "Run this locally", the YAML card fills in, and the
   releases page is one click away.

### Layout (desktop; single column stacking on mobile)

```text
+----------------------------------------------------------------------+
|  sse - stochastic series expansion            [docs] [github] [dark] |
+------------------------------+---------------------------------------+
|  MODEL      [TFIM | Rydberg] |                                       |
|                              |        lattice preview (SVG)          |
|  LATTICE                     |     o--o--o--o--o--o                  |
|   size   [4x4 5x5 6x6 | +]   |     |  |  |  |  |  |   wrap arcs      |
|   bounds [open|periodic] x/y |     o--o--o--o--o--o   when periodic  |
|                              |                                       |
|  PARAMETERS                  +---------------------------------------+
|   h/J        [ 3.044  ] o--- |  RESULT                               |
|   beta       [ auto=2L ]     |   E/site = -3.20514 +/- 0.00042       |
|   chains     [ 8 ]           |   [R-hat 1.001 ok] [ESS 9.4k ok]      |
|   seed       [ 24301 ] [??]  |   C/site = 0.0113 +/- 0.0009          |
|                              |                                       |
|  [ Run in browser ]          |   convergence plot: E vs sweeps,      |
|   est. ~20 s on this device  |   one faint line per chain, shaded    |
|                              |   +/- SE band, beta-ladder inset      |
|  ---------------------------- |                                      |
|  REPRODUCE LOCALLY           |   chain table: E/site, ESS, tau       |
|   [yaml] [copy] [releases]   |                                       |
+------------------------------+---------------------------------------+
```

### Interaction flows

- **First load:** a curated preset (4x4 TFIM at criticality) is prefilled;
  one click gives a first result in seconds. Empty states never appear.
- **Running:** the Run button morphs into a progress bar segmented per
  chain; the result panel shows the live estimate updating each batch with
  an aria-live region announcing milestones. Cancel is always visible.
- **Done:** the estimate locks in, badges compute, and the URL hash updates
  to encode the full configuration - every result is a shareable deep link.
  Previous runs of the session stack in a small history rail (localStorage)
  for side-by-side comparison.
- **Ground-state honesty:** "beta = auto" runs a short ladder (three beta
  values) and the inset plot shows E(beta) flattening; the headline number
  is the largest-beta point, labeled "ground-state estimate (beta = 12)".
- **Handoff:** the YAML card always mirrors the current form. At >6x6 (or
  Rydberg past its budget) the primary CTA becomes:
  `curl -L <release-url> ... && sse run --config run.yaml --output run/`
  with platform auto-detected from the user agent.

### Visual language

- Typography: system font stack; all numerals tabular (`font-variant-
  numeric: tabular-nums`) so live-updating digits do not jitter; results in
  a monospaced face.
- Color: near-white and near-black themes driven by `prefers-color-scheme`
  with a manual toggle; one accent color for interactive elements; chain
  traces in a muted categorical ramp; error bands as translucent fills.
  All pairs checked to WCAG AA in both themes.
- Motion: the convergence trace animates by data arrival only - no
  decorative animation; `prefers-reduced-motion` collapses transitions.
- Accessibility: full keyboard operation, visible focus, labeled controls,
  aria-live progress, and plot data mirrored in the chain table so no
  information lives only in pixels.

## 7. Reproducibility contract (what we promise publicly)

- The footer of every result: `sse-web {version} / qslib-quantum {version}
  / seed scheme qslib-seed-v1 / master seed {n}`.
- Promise: identical configuration + identical versions => identical chain
  series, browser or native. Enforced by CI (below), not by hope.
- The YAML export is the canonical `sse-run-v1` document the CLI validates;
  round-trip is tested.

## 8. CI/CD

Extend `pages.yml`:

1. `cargo test --workspace --locked` (existing gates now workspace-wide).
2. **Parity gate:** run one short fixed-seed chain natively (test in
   `sse-web` compiled for the host) and the same chain in the wasm module
   under `wasmtime`; assert the expansion-order series are identical. This
   is the determinism promise as a failing test.
3. `wasm-pack build --release` (or `cargo build --target wasm32-unknown-
   unknown` + `wasm-bindgen` + `wasm-opt -Oz`), enforce the 400 KB gzip
   budget with a hard check.
4. Assemble `target/site = mdBook + api docs + demo/`, deploy as today.

## 9. Milestones

- **M1 - engine in wasm (1-2 days):** workspace conversion; `sse-web`
  crate with `create_chain`/`advance`; parity gate green in CI.
- **M2 - walking skeleton (1 day):** unstyled page, worker pool, form to
  number with error bar. Deployed behind `/demo/`.
- **M3 - the experience (2-3 days):** lattice preview, live convergence
  plot, badges, beta ladder, YAML/handoff card, URL sharing, themes,
  accessibility pass, mobile layout.
- **M4 - launch (1 day):** precomputed reference grid for preset points
  (instant answers, live compute for custom parameters), mdBook chapter,
  README section, link from the qslib docs. Tag as 0.3.0.

## 10. Risks

| Risk | Mitigation |
| --- | --- |
| wasm/native float divergence would falsify the parity promise | The CI parity gate is the promise; if a platform ever diverges we find out in CI, not from a user |
| Rydberg runtimes disappoint online | Conservative online budget + visible ETA + first-class local handoff |
| Safari worker/module quirks | Workers use classic scripts + `importScripts` fallback; test matrix includes WebKit |
| Scope creep toward a physics IDE | Non-goals section above; new observables belong in qslib first |
