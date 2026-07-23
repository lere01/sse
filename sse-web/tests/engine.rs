//! Conformance tests for the browser chain engine.
//!
//! The parity test is the product's central promise: a chain advanced by
//! the web engine reproduces the CLI runner's expansion-order series bit
//! for bit for the same configuration, seed scheme, and version.

use serde::Deserialize;
use sse_web::config::RunConfig;
use sse_web::engine::{ChainEngine, Phase};

#[derive(Deserialize)]
struct ParityFixture {
    config_yaml: String,
    chains: Vec<ParityChain>,
}

#[derive(Deserialize)]
struct ParityChain {
    chain_index: u64,
    seed: String,
    expansion_orders: Vec<usize>,
}

fn run_to_completion(config: &RunConfig, chain_index: u64, batch: usize) -> Vec<usize> {
    let mut engine = ChainEngine::new(config, chain_index).unwrap();
    let mut orders = Vec::new();
    loop {
        let report = engine.advance(batch).unwrap();
        orders.extend(report.orders);
        if report.phase == Phase::Complete {
            return orders;
        }
    }
}

#[test]
fn web_chains_reproduce_the_cli_runner_series_exactly() {
    let fixture: ParityFixture =
        serde_json::from_str(include_str!("fixtures/cli_parity.json")).unwrap();
    let config = RunConfig::from_yaml_str(&fixture.config_yaml).unwrap();
    for chain in &fixture.chains {
        let orders = run_to_completion(&config, chain.chain_index, 37);
        assert_eq!(
            orders, chain.expansion_orders,
            "chain {} diverged from the CLI series",
            chain.chain_index
        );
        assert_eq!(chain.seed.len(), 64, "fixture seeds are 32-byte hex");
    }
}

#[test]
fn batching_does_not_change_the_series() {
    let fixture: ParityFixture =
        serde_json::from_str(include_str!("fixtures/cli_parity.json")).unwrap();
    let config = RunConfig::from_yaml_str(&fixture.config_yaml).unwrap();
    let coarse = run_to_completion(&config, 0, 10_000);
    let fine = run_to_completion(&config, 0, 1);
    assert_eq!(coarse, fine);
}

#[test]
fn advance_reports_phases_and_counts() {
    let fixture: ParityFixture =
        serde_json::from_str(include_str!("fixtures/cli_parity.json")).unwrap();
    let config = RunConfig::from_yaml_str(&fixture.config_yaml).unwrap();
    let mut engine = ChainEngine::new(&config, 0).unwrap();
    let report = engine.advance(10).unwrap();
    assert_eq!(report.phase, Phase::Thermalization);
    assert_eq!(report.thermalization_remaining, 40);
    assert!(report.orders.is_empty());
    let report = engine.advance(40).unwrap();
    assert_eq!(report.phase, Phase::Measurement);
    // Two sweeps per measurement: 100 more sweeps yields 50 measurements.
    let report = engine.advance(100).unwrap();
    assert_eq!(report.orders.len(), 50);
    assert_eq!(report.measurements_remaining, 150);
    assert!(report.num_sites == 4 && report.energy_shift > 0.0);
    assert!(report.diagonal.insertions_proposed > 0);
}

#[test]
fn rydberg_chains_run_and_complete() {
    let yaml = r#"
schema_version: sse-run-v1
name: web rydberg smoke
model:
  kind: rydberg
  geometry:
    kind: rectangular
    lx: 2
    ly: 2
    boundary_x: open
    boundary_y: open
  omega: 1.0
  detuning: 1.0
  c6: 1.0
simulation:
  beta: 2.0
  operator_string_length: 64
  thermalization_sweeps: 100
  measurement_sweeps: 200
execution:
  chains: 1
  threads: 1
  seed: 5
initial_state: down
"#;
    let config = RunConfig::from_yaml_str(yaml).unwrap();
    let orders = run_to_completion(&config, 0, 61);
    assert_eq!(orders.len(), 200);
    assert!(orders.iter().any(|&order| order > 0));
}

#[test]
fn yaml_round_trip_preserves_the_cli_schema() {
    let fixture: ParityFixture =
        serde_json::from_str(include_str!("fixtures/cli_parity.json")).unwrap();
    let config = RunConfig::from_yaml_str(&fixture.config_yaml).unwrap();
    let yaml = config.to_yaml_string().unwrap();
    assert_eq!(RunConfig::from_yaml_str(&yaml).unwrap(), config);
    let json = serde_json::to_string(&config).unwrap();
    let from_json: RunConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(from_json, config);
}
