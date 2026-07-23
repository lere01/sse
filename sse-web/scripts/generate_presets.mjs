// Generates sse-web/web/presets.json by running the native CLI for the
// page's preset configurations. Keys must match app.js presetKey() exactly;
// the duplication is deliberate and covered by the note in each entry.
//
// Usage: node sse-web/scripts/generate_presets.mjs [path-to-sse-binary]

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..");
const binary = process.argv[2] ?? join(repo, "target", "release", "sse");

// Mirror of app.js defaults: clicking a size chip leaves every other field
// at its default, and beta-auto resolves the headline beta to 2 * max(L).
const defaults = {
  j: 1,
  h: 3.044,
  chains: 8,
  sweeps: 10000,
  seed: 24301,
};

const points = [
  { lx: 4, ly: 4 },
  { lx: 5, ly: 5 },
  { lx: 6, ly: 6 },
  { lx: 16, ly: 1 },
];

function configFor(point) {
  const beta = 2 * Math.max(point.lx, point.ly);
  const geometry =
    point.ly === 1
      ? { kind: "chain", length: point.lx, boundary: "periodic" }
      : {
          kind: "rectangular",
          lx: point.lx,
          ly: point.ly,
          boundary_x: "periodic",
          boundary_y: "periodic",
        };
  return {
    schema_version: "sse-run-v1",
    name: `browser tfim ${point.lx}x${point.ly}`,
    model: { kind: "tfim", geometry, j: defaults.j, h: defaults.h },
    simulation: {
      beta,
      operator_string_length: 64,
      thermalization_sweeps: Math.max(1000, Math.round(defaults.sweeps / 10)),
      measurement_sweeps: defaults.sweeps,
      sweeps_per_measurement: 1,
    },
    execution: {
      chains: defaults.chains,
      threads: defaults.chains,
      seed: defaults.seed,
    },
    initial_state: "down",
  };
}

// Must stay identical to presetKey() in app.js.
function presetKey(config) {
  const model = config.model;
  const geometry = model.geometry;
  const shape =
    geometry.kind === "chain"
      ? `chain${geometry.length}-${geometry.boundary}`
      : `${geometry.lx}x${geometry.ly}-${geometry.boundary_x}${geometry.boundary_y}`;
  const params =
    model.kind === "tfim"
      ? `j${model.j}-h${model.h}`
      : `o${model.omega}-d${model.detuning}-c${model.c6}`;
  const sim = config.simulation;
  return `${model.kind}-${shape}-${params}-b${sim.beta}-m${sim.measurement_sweeps}-s${config.execution.seed}-ch${config.execution.chains}`;
}

function toYaml(config) {
  const geometry = config.model.geometry;
  const geometryYaml =
    geometry.kind === "chain"
      ? `    kind: chain\n    length: ${geometry.length}\n    boundary: ${geometry.boundary}`
      : `    kind: rectangular\n    lx: ${geometry.lx}\n    ly: ${geometry.ly}\n    boundary_x: ${geometry.boundary_x}\n    boundary_y: ${geometry.boundary_y}`;
  return `schema_version: sse-run-v1
name: ${config.name}
model:
  kind: tfim
  geometry:
${geometryYaml}
  j: ${config.model.j.toFixed(1)}
  h: ${config.model.h}
simulation:
  beta: ${config.simulation.beta.toFixed(1)}
  operator_string_length: ${config.simulation.operator_string_length}
  thermalization_sweeps: ${config.simulation.thermalization_sweeps}
  measurement_sweeps: ${config.simulation.measurement_sweeps}
execution:
  chains: ${config.execution.chains}
  threads: ${config.execution.threads}
  seed: ${config.execution.seed}
initial_state: down
`;
}

const references = {};
for (const point of points) {
  const config = configFor(point);
  const workDir = mkdtempSync(join(tmpdir(), "sse-preset-"));
  const configPath = join(workDir, "run.yaml");
  const outputPath = join(workDir, "out");
  writeFileSync(configPath, toYaml(config));
  console.log(`running ${point.lx}x${point.ly} (beta ${config.simulation.beta}) ...`);
  execFileSync(binary, [
    "run",
    "--config",
    configPath,
    "--output",
    outputPath,
    "--quiet",
  ]);
  const summary = JSON.parse(readFileSync(join(outputPath, "summary.json")));
  references[presetKey(config)] = {
    energy_per_site: summary.energy_per_site,
    standard_error: summary.chain_standard_error,
    split_r_hat: summary.split_r_hat,
    version: "0.2.0",
  };
  console.log(
    `  E/site = ${summary.energy_per_site.toFixed(8)} ± ${summary.chain_standard_error.toExponential(2)}`,
  );
  rmSync(workDir, { recursive: true, force: true });
}

const output = {
  generated_by: "sse-web/scripts/generate_presets.mjs (native sse CLI)",
  references,
};
writeFileSync(
  join(here, "..", "web", "presets.json"),
  `${JSON.stringify(output, null, 1)}\n`,
);
console.log(`wrote ${Object.keys(references).length} references to presets.json`);
