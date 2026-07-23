// End-to-end headless check of the page's aggregation pipeline: run the
// fixture chains through the real wasm module, aggregate with ui/stats.js,
// and compare every reported statistic against the CLI's own summary.json
// values (which come from qslib-quantum-variational in Rust).
//
// Usage: node sse-web/tests/stats_parity.mjs

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  autocorrelation,
  chainThermodynamics,
  combine,
  runningEnergyTrace,
  splitRHat,
} from "../web/ui/stats.js";

const here = dirname(fileURLToPath(import.meta.url));
const pkg = join(here, "..", "web", "pkg");
const { default: init, ChainHandle } = await import(join(pkg, "sse_web.js"));
await init({ module_or_path: readFileSync(join(pkg, "sse_web_bg.wasm")) });

const fixture = JSON.parse(
  readFileSync(join(here, "fixtures", "cli_parity.json"), "utf8"),
);
const expected = fixture.cli_summary;

const config = {
  schema_version: "sse-run-v1",
  name: "web parity fixture",
  model: {
    kind: "tfim",
    geometry: { kind: "chain", length: 4, boundary: "periodic" },
    j: 1.0,
    h: 0.5,
  },
  simulation: {
    beta: 2.0,
    operator_string_length: 32,
    thermalization_sweeps: 50,
    measurement_sweeps: 200,
    sweeps_per_measurement: 2,
  },
  execution: { chains: 2, threads: 1, seed: 24301 },
  initial_state: "alternating",
};
const beta = config.simulation.beta;

function close(actual, wanted, tolerance, label) {
  if (Math.abs(actual - wanted) > tolerance) {
    throw new Error(`${label}: ${actual} != ${wanted} (tol ${tolerance})`);
  }
}

const chains = [];
let energyShift = null;
let numSites = null;
for (const { chain_index: chainIndex } of fixture.chains) {
  const handle = new ChainHandle(JSON.stringify(config), chainIndex);
  const orders = [];
  for (;;) {
    const report = JSON.parse(handle.advance(500));
    orders.push(...report.orders);
    energyShift = report.energy_shift;
    numSites = report.num_sites;
    if (report.phase === "complete") break;
  }
  chains.push(orders);
}

// Per-chain thermodynamics and diagnostics against the CLI chain artifacts.
chains.forEach((orders, index) => {
  const thermo = chainThermodynamics(orders, beta, energyShift, numSites);
  const diagnostics = expected.chain_diagnostics[index];
  close(thermo.energyPerSite, diagnostics.energy_per_site, 1e-12, `chain ${index} E/site`);
  close(
    thermo.heatCapacityPerSite,
    diagnostics.heat_capacity_per_site,
    1e-9,
    `chain ${index} C/site`,
  );
  const acf = autocorrelation(orders);
  close(
    acf.tau,
    diagnostics.integrated_autocorrelation_time,
    1e-9,
    `chain ${index} tau`,
  );
  close(
    acf.ess,
    diagnostics.effective_sample_size,
    1e-6,
    `chain ${index} ESS`,
  );
});

// Combined estimate and split R-hat against summary.json.
const energies = chains.map(
  (orders) => chainThermodynamics(orders, beta, energyShift, numSites).energyPerSite,
);
const combined = combine(energies);
close(combined.mean, expected.energy_per_site, 1e-12, "combined E/site");
close(
  combined.standardError,
  expected.chain_standard_error,
  1e-12,
  "between-chain SE",
);
close(splitRHat(chains), expected.split_r_hat, 1e-9, "split R-hat");

// The convergence trace must end at the chain mean.
const trace = runningEnergyTrace(chains[0], beta, energyShift, numSites);
close(
  trace[trace.length - 1].energyPerSite,
  energies[0],
  1e-12,
  "running trace endpoint",
);

console.log(
  "stats parity OK: JS aggregation matches the CLI summary " +
    `(E/site ${combined.mean.toFixed(9)} ± ${combined.standardError.toExponential(3)}, ` +
    `R-hat ${splitRHat(chains).toFixed(6)})`,
);
