//! Browser surface for the `sse` CLI: the same qslib SSE engine, the same
//! configuration schema, and the same deterministic chain streams, compiled
//! to WebAssembly and driven in resumable batches from Web Workers.
//!
//! The configuration and model-resolution modules are included from the CLI
//! crate's source by path, so the browser and the binary share one contract
//! by construction: a run configured here serializes to exactly the
//! `sse-run-v1` YAML the CLI validates, and a chain advanced here follows
//! exactly the CLI runner's sweep sequence.

// Shared source with the CLI binary. `config` refers to `crate::model`
// internally, which resolves to the sibling module below in both crates.
#[path = "../../src/model.rs"]
pub mod model;

#[path = "../../src/config.rs"]
pub mod config;

pub mod engine;

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::*;
