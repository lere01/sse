// Cross-validates presets.json against the repository's historical
// periodic-lattice benchmark log (tfim_periodic_boundary.txt), which was
// produced by the pre-qslib standalone engine with a different RNG. Every
// overlapping point (same size, h, beta) must agree within combined
// statistical error - an engine-independent physics check.
//
// Usage: node sse-web/scripts/check_presets_history.mjs

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..");

const presets = JSON.parse(
  readFileSync(join(here, "..", "web", "presets.json"), "utf8"),
).references;

const history = new Map();
for (const line of readFileSync(join(repo, "tfim_periodic_boundary.txt"), "utf8")
  .split("\n")
  .slice(1)) {
  const parts = line.split(",").map((part) => part.trim());
  if (parts.length < 5) continue;
  const [size, h, beta, energy, error] = parts;
  history.set(`${size}-h${Number(h)}-b${Number(beta)}`, {
    energy: Number(energy),
    error: Number(error),
  });
}

let compared = 0;
let failures = 0;
for (const [key, preset] of Object.entries(presets)) {
  const match = key.match(/^tfim-(\d+)x(\d+)-periodicperiodic-j1-h([\d.]+)-b(\d+)-/);
  if (!match) continue;
  const [, lx, ly, h, beta] = match;
  if (lx !== ly) continue;
  const reference = history.get(`${lx}x${ly}-h${Number(h)}-b${Number(beta)}`);
  if (!reference) continue;
  compared += 1;
  const difference = Math.abs(preset.energy_per_site - reference.energy);
  const combined = Math.hypot(preset.standard_error, reference.error);
  const sigma = difference / combined;
  const verdict = sigma < 3 ? "ok " : "FAIL";
  if (sigma >= 3) failures += 1;
  console.log(
    `${verdict} ${lx}x${ly} h=${h}: new ${preset.energy_per_site.toFixed(6)} ` +
      `vs legacy ${reference.energy.toFixed(6)} (${sigma.toFixed(2)}σ)`,
  );
}

if (compared === 0) throw new Error("no overlapping points found");
if (failures > 0) throw new Error(`${failures}/${compared} points disagree beyond 3σ`);
console.log(`history cross-check OK: ${compared} points agree within 3σ`);
