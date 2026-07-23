// Runs the compiled WebAssembly module under Node and asserts that its
// chain series match the CLI-generated parity fixture exactly. This is the
// cross-target half of the determinism promise: native Rust and the wasm
// binary produce identical streams.
//
// Usage: node sse-web/tests/wasm_parity.mjs

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, "..", "web", "pkg");

const { default: init, ChainHandle, config_yaml, validate_config } =
  await import(join(pkg, "sse_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "sse_web_bg.wasm")) });

const fixture = JSON.parse(
  readFileSync(join(here, "fixtures", "cli_parity.json"), "utf8"),
);

// The fixture carries YAML; the wasm boundary speaks JSON. Convert with a
// tiny literal parser for exactly the fixture's flat structure.
function yamlToConfig(yaml) {
  const lines = yaml.split("\n");
  const config = {
    schema_version: "sse-run-v1",
    name: "web parity fixture",
    model: {
      kind: "tfim",
      geometry: { kind: "chain", length: 4, boundary: "periodic" },
      j: 1.0,
      h: 0.5,
    },
    simulation: {},
    execution: {},
    initial_state: "alternating",
  };
  const scalar = (key) => {
    const line = lines.find((l) => l.trim().startsWith(`${key}:`));
    return line ? line.split(":")[1].trim() : undefined;
  };
  config.simulation = {
    beta: Number(scalar("beta")),
    operator_string_length: Number(scalar("operator_string_length")),
    thermalization_sweeps: Number(scalar("thermalization_sweeps")),
    measurement_sweeps: Number(scalar("measurement_sweeps")),
    sweeps_per_measurement: Number(scalar("sweeps_per_measurement")),
  };
  config.execution = {
    chains: Number(scalar("chains")),
    threads: Number(scalar("threads")),
    seed: Number(scalar("seed")),
  };
  return config;
}

const config = yamlToConfig(fixture.config_yaml);
const configJson = JSON.stringify(config);

const summary = JSON.parse(validate_config(configJson));
if (summary.num_sites !== 4) throw new Error("unexpected validate summary");

// YAML round trip: the wasm-exported YAML must contain the schema marker.
if (!config_yaml(configJson).includes("schema_version: sse-run-v1")) {
  throw new Error("config_yaml lost the schema version");
}

let checked = 0;
for (const chain of fixture.chains) {
  const handle = new ChainHandle(configJson, chain.chain_index);
  const orders = [];
  for (;;) {
    const report = JSON.parse(handle.advance(41));
    orders.push(...report.orders);
    if (report.phase === "complete") break;
  }
  const expected = chain.expansion_orders;
  if (orders.length !== expected.length) {
    throw new Error(
      `chain ${chain.chain_index}: length ${orders.length} != ${expected.length}`,
    );
  }
  for (let i = 0; i < expected.length; i += 1) {
    if (orders[i] !== expected[i]) {
      throw new Error(
        `chain ${chain.chain_index} diverged at measurement ${i}: ` +
          `wasm ${orders[i]} != cli ${expected[i]}`,
      );
    }
  }
  checked += 1;
}

console.log(
  `wasm parity OK: ${checked} chains identical to the CLI fixture ` +
    `(${fixture.chains[0].expansion_orders.length} measurements each)`,
);
