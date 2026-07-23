//! `wasm-bindgen` bindings over the resumable chain engine.
//!
//! The boundary is JSON both ways: the page submits a complete `sse-run-v1`
//! configuration object, and `advance` returns an incremental report. All
//! aggregation (chain means, errors, R-hat) happens in the page.

use wasm_bindgen::prelude::*;

use crate::config::RunConfig;
use crate::engine::ChainEngine;

/// Largest lattice the hosted page will simulate in a browser tab.
const MAX_WEB_SITES: usize = 36;
/// Upper bound on requested chains, to keep worker fan-out sane.
const MAX_WEB_CHAINS: usize = 16;

fn parse_config(config_json: &str) -> Result<RunConfig, JsError> {
    let config: RunConfig =
        serde_json::from_str(config_json).map_err(|error| JsError::new(&error.to_string()))?;
    config
        .validate()
        .map_err(|error| JsError::new(&error.to_string()))?;
    Ok(config)
}

fn enforce_web_limits(config: &RunConfig) -> Result<usize, JsError> {
    let geometry = config
        .model
        .geometry()
        .build()
        .map_err(|error| JsError::new(&error.to_string()))?;
    let num_sites = geometry.num_sites();
    if num_sites > MAX_WEB_SITES {
        return Err(JsError::new(&format!(
            "browser runs are limited to {MAX_WEB_SITES} sites; got {num_sites}. \
             Export the configuration and run it with the installed sse binary."
        )));
    }
    if config.execution.chains > MAX_WEB_CHAINS {
        return Err(JsError::new(&format!(
            "browser runs are limited to {MAX_WEB_CHAINS} chains"
        )));
    }
    Ok(num_sites)
}

/// Validates a configuration and returns a JSON summary for the page.
#[wasm_bindgen]
pub fn validate_config(config_json: &str) -> Result<String, JsError> {
    let config = parse_config(config_json)?;
    let num_sites = enforce_web_limits(&config)?;
    let summary = serde_json::json!({
        "num_sites": num_sites,
        "model": config.model.kind_name(),
        "chains": config.execution.chains,
        "beta": config.simulation.beta,
        "measurement_sweeps": config.simulation.measurement_sweeps,
    });
    Ok(summary.to_string())
}

/// Serializes a configuration as canonical `sse-run-v1` YAML for the CLI.
#[wasm_bindgen]
pub fn config_yaml(config_json: &str) -> Result<String, JsError> {
    let config = parse_config(config_json)?;
    config
        .to_yaml_string()
        .map_err(|error| JsError::new(&error.to_string()))
}

/// One independent, resumable Monte Carlo chain owned by a Web Worker.
#[wasm_bindgen]
pub struct ChainHandle {
    engine: ChainEngine,
}

#[wasm_bindgen]
impl ChainHandle {
    /// Builds the chain for `chain_index` under the web size limits.
    #[wasm_bindgen(constructor)]
    pub fn new(config_json: &str, chain_index: u32) -> Result<ChainHandle, JsError> {
        let config = parse_config(config_json)?;
        enforce_web_limits(&config)?;
        let engine = ChainEngine::new(&config, u64::from(chain_index))
            .map_err(|error| JsError::new(&error))?;
        Ok(ChainHandle { engine })
    }

    /// Runs up to `sweeps` sweeps and returns the JSON advance report.
    pub fn advance(&mut self, sweeps: u32) -> Result<String, JsError> {
        let report = self
            .engine
            .advance(sweeps as usize)
            .map_err(|error| JsError::new(&error))?;
        serde_json::to_string(&report).map_err(|error| JsError::new(&error.to_string()))
    }
}
