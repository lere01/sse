// sse-web application shell: form state, worker-pool orchestration, live
// aggregation, and the reproduce-locally handoff. All physics runs in the
// wasm module; all statistics live in ui/stats.js; this file wires them to
// the page.

import init, { config_yaml, validate_config } from "./pkg/sse_web.js";
import {
  autocorrelation,
  chainThermodynamics,
  combine,
  runningEnergyTrace,
  splitRHat,
} from "./ui/stats.js";
import { renderLattice } from "./ui/lattice.js";
import { renderConvergence, renderLadder } from "./ui/plot.js";

const MAX_WEB_SITES = 36;
const MAX_RYDBERG_WEB_SITES = 16;
const APP_VERSION = "0.2.0";

const el = (id) => document.getElementById(id);
const runButton = el("run");

let wasmReady = false;
let activeCampaign = null;
let presets = new Map();

// ---------------------------------------------------------------- form state

function readForm() {
  const model = el("model-tfim").classList.contains("selected") ? "tfim" : "rydberg";
  const lx = clampInt(el("lx").value, 1, 64);
  const ly = clampInt(el("ly").value, 1, 64);
  return {
    model,
    lx,
    ly,
    periodicX: el("periodic-x").checked,
    periodicY: el("periodic-y").checked,
    j: Number(el("tfim-j").value),
    h: Number(el("tfim-h").value),
    omega: Number(el("ryd-omega").value),
    detuning: Number(el("ryd-detuning").value),
    c6: Number(el("ryd-c6").value),
    betaAuto: el("beta-auto").checked,
    beta: Number(el("beta").value),
    chains: clampInt(el("chains").value, 2, 16),
    sweeps: clampInt(el("sweeps").value, 100, 500000),
    seed: clampInt(el("seed").value, 0, Number.MAX_SAFE_INTEGER),
  };
}

function clampInt(value, low, high) {
  const parsed = Math.round(Number(value));
  if (!Number.isFinite(parsed)) return low;
  return Math.min(high, Math.max(low, parsed));
}

function numSites(ui) {
  return ui.lx * ui.ly;
}

function largestDimension(ui) {
  return Math.max(ui.lx, ui.ly);
}

function ladderBetas(ui) {
  const scale = largestDimension(ui);
  return [scale, 1.5 * scale, 2 * scale].map((value) => Math.round(value * 2) / 2);
}

function headlineBeta(ui) {
  return ui.betaAuto ? 2 * largestDimension(ui) : ui.beta;
}

function isLocalOnly(ui) {
  const sites = numSites(ui);
  if (sites > MAX_WEB_SITES) return true;
  return ui.model === "rydberg" && sites > MAX_RYDBERG_WEB_SITES;
}

/** Builds the exact sse-run-v1 configuration object the CLI validates. */
function buildRunConfig(ui, beta, measurementSweeps) {
  const geometry =
    ui.ly === 1
      ? { kind: "chain", length: ui.lx, boundary: ui.periodicX ? "periodic" : "open" }
      : {
          kind: "rectangular",
          lx: ui.lx,
          ly: ui.ly,
          boundary_x: ui.periodicX ? "periodic" : "open",
          boundary_y: ui.periodicY ? "periodic" : "open",
        };
  const model =
    ui.model === "tfim"
      ? { kind: "tfim", geometry, j: ui.j, h: ui.h }
      : {
          kind: "rydberg",
          geometry,
          omega: ui.omega,
          detuning: ui.detuning,
          c6: ui.c6,
          update: "local",
        };
  return {
    schema_version: "sse-run-v1",
    name: `browser ${ui.model} ${ui.lx}x${ui.ly}`,
    model,
    simulation: {
      beta,
      operator_string_length: 64,
      thermalization_sweeps: Math.max(1000, Math.round(measurementSweeps / 10)),
      measurement_sweeps: measurementSweeps,
      sweeps_per_measurement: 1,
    },
    execution: { chains: ui.chains, threads: ui.chains, seed: ui.seed },
    initial_state: "down",
  };
}

// ------------------------------------------------------------------- ui sync

function syncAll() {
  const ui = readForm();
  el("periodic-y-wrap").classList.toggle("hidden", ui.ly === 1);
  el("params-tfim").classList.toggle("hidden", ui.model !== "tfim");
  el("params-rydberg").classList.toggle("hidden", ui.model !== "rydberg");
  el("beta").disabled = ui.betaAuto;
  el("beta-manual-wrap").classList.toggle("dimmed", ui.betaAuto);

  renderLattice(el("lattice"), {
    lx: ui.lx,
    ly: ui.ly,
    periodicX: ui.periodicX,
    periodicY: ui.periodicY && ui.ly > 1,
  });
  const shape = ui.ly === 1 ? `chain of ${ui.lx}` : `${ui.lx}×${ui.ly} square`;
  const bounds =
    ui.ly === 1
      ? ui.periodicX ? "periodic" : "open"
      : ui.periodicX === ui.periodicY
        ? (ui.periodicX ? "periodic" : "open")
        : `${ui.periodicX ? "periodic" : "open"} x / ${ui.periodicY ? "periodic" : "open"} y`;
  el("lattice-caption").textContent = `${shape}, ${bounds}`;
  el("site-count").textContent = `${numSites(ui)} sites`;

  const localOnly = isLocalOnly(ui);
  runButton.textContent = localOnly ? "Run locally with the sse CLI" : "Run in browser";
  runButton.classList.toggle("local-mode", localOnly);
  el("run-hint").textContent = localOnly
    ? numSites(ui) > MAX_WEB_SITES
      ? `${numSites(ui)} sites is beyond the ${MAX_WEB_SITES}-site browser limit — the YAML below reproduces this run natively.`
      : `Rydberg beyond ${MAX_RYDBERG_WEB_SITES} sites is slow in a tab — the YAML below runs it natively.`
    : "";

  updateYamlCard(ui);
  updatePresetNote(ui);
}

async function updateYamlCard(ui) {
  const config = buildRunConfig(ui, headlineBeta(ui), ui.sweeps);
  if (!wasmReady) {
    el("yaml").textContent = "… loading wasm module";
    return;
  }
  try {
    el("yaml").textContent = config_yaml(JSON.stringify(config));
  } catch (error) {
    el("yaml").textContent = `invalid configuration: ${error.message ?? error}`;
  }
}

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

function updatePresetNote(ui) {
  const config = buildRunConfig(ui, headlineBeta(ui), ui.sweeps);
  const reference = presets.get(presetKey(config));
  const existing = document.getElementById("preset-note");
  if (existing) existing.remove();
  if (!reference) return;
  const note = document.createElement("p");
  note.id = "preset-note";
  note.className = "hint";
  note.innerHTML = `Reference for this exact configuration, computed natively with sse ${reference.version}: <strong>E/site = ${reference.energy_per_site.toFixed(6)} ± ${reference.standard_error.toExponential(2)}</strong>. Running in the browser reproduces it bit for bit.`;
  el("results-idle").append(note.cloneNode(true));
  el("results-live").insertBefore(note, el("status-line"));
}

// ------------------------------------------------------------- worker runner

class ChainRun {
  constructor(config, chainCount) {
    this.config = config;
    this.orders = Array.from({ length: chainCount }, () => []);
    this.progress = Array.from({ length: chainCount }, () => 0);
    this.totalSweepsPerChain =
      config.simulation.thermalization_sweeps +
      config.simulation.measurement_sweeps * config.simulation.sweeps_per_measurement;
    this.energyShift = null;
    this.numSites = null;
    this.completed = new Set();
    this.cancelled = false;
    this.sweepsDone = 0;
    this.startedAt = performance.now();
  }
}

function runOnce(config, phaseLabel, onLive) {
  let cancelHook = () => {};
  const promise = new Promise((resolve) => {
    const chainCount = config.execution.chains;
    const run = new ChainRun(config, chainCount);
    const poolSize = Math.min(
      chainCount,
      Math.max(1, (navigator.hardwareConcurrency || 4) - 1),
      8,
    );
    const workers = [];
    const queue = Array.from({ length: chainCount }, (_, index) => index);
    const workerChain = new Map();
    const batch = new Map();
    let paused = document.hidden;
    const pendingTicks = [];

    const finish = () => {
      workers.forEach((worker) => worker.terminate());
      document.removeEventListener("visibilitychange", onVisibility);
      resolve(run);
    };

    const onVisibility = () => {
      paused = document.hidden;
      if (!paused) {
        while (pendingTicks.length) pendingTicks.shift()();
        onLive(run, `${phaseLabel} — resumed`);
      } else {
        onLive(run, `${phaseLabel} — paused (tab hidden)`);
      }
    };
    document.addEventListener("visibilitychange", onVisibility);

    run.cancel = () => {
      run.cancelled = true;
      finish();
    };
    cancelHook = run.cancel;

    const tick = (worker) => {
      const send = () =>
        worker.postMessage({ type: "tick", sweeps: batch.get(worker) ?? 200 });
      if (paused) pendingTicks.push(send);
      else send();
    };

    const assign = (worker) => {
      const next = queue.shift();
      if (next === undefined) {
        workerChain.delete(worker);
        if (run.completed.size === chainCount) finish();
        return;
      }
      workerChain.set(worker, next);
      worker.postMessage({ type: "create", config, chainIndex: next });
    };

    for (let i = 0; i < poolSize; i += 1) {
      const worker = new Worker(new URL("./worker.js", import.meta.url), {
        type: "module",
      });
      workers.push(worker);
      batch.set(worker, 200);
      worker.onmessage = (event) => {
        if (run.cancelled) return;
        const message = event.data;
        if (message.type === "ready") {
          assign(worker);
        } else if (message.type === "created") {
          tick(worker);
        } else if (message.type === "report") {
          const chainIndex = message.chainIndex;
          const report = message.report;
          run.energyShift = report.energy_shift;
          run.numSites = report.num_sites;
          run.orders[chainIndex].push(...report.orders);
          run.sweepsDone += message.sweeps;
          const remaining =
            report.thermalization_remaining +
            report.measurements_remaining * config.simulation.sweeps_per_measurement;
          run.progress[chainIndex] =
            1 - remaining / run.totalSweepsPerChain;
          // Adapt the batch toward ~180 ms of work.
          const scaled = Math.round(
            (message.sweeps * 180) / Math.max(message.elapsedMs, 8),
          );
          batch.set(worker, Math.min(20000, Math.max(50, scaled)));
          if (report.phase === "complete") {
            run.completed.add(chainIndex);
            worker.postMessage({ type: "dispose" });
            assign(worker);
          } else {
            tick(worker);
          }
          onLive(run, phaseLabel);
        } else if (message.type === "error") {
          console.error("worker error", message);
          onLive(run, `error: ${message.message}`);
          run.error = message.message;
          run.cancelled = true;
          finish();
        }
      };
      worker.postMessage({
        type: "init",
        wasmUrl: new URL("./pkg/sse_web_bg.wasm", import.meta.url).href,
      });
    }
  });
  return { promise, cancel: () => cancelHook() };
}

// ------------------------------------------------------------- live rendering

let renderQueued = false;

function scheduleRender(run, label) {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    renderLive(run, label);
  });
}

function renderLive(run, label) {
  const rail = el("progress-rail");
  if (rail.childElementCount !== run.progress.length) {
    rail.replaceChildren(
      ...run.progress.map(() => {
        const chunk = document.createElement("span");
        chunk.className = "progress-chunk";
        chunk.append(document.createElement("i"));
        return chunk;
      }),
    );
  }
  run.progress.forEach((fraction, index) => {
    rail.children[index].firstChild.style.width = `${Math.round(fraction * 100)}%`;
  });

  const beta = run.config.simulation.beta;
  const measured = run.orders.filter((orders) => orders.length > 8);
  if (measured.length && run.energyShift !== null) {
    const energies = measured.map(
      (orders) =>
        chainThermodynamics(orders, beta, run.energyShift, run.numSites).energyPerSite,
    );
    const combined = combine(energies);
    el("headline-value").textContent = formatEnergy(combined);
    const traces = run.orders
      .filter((orders) => orders.length > 1)
      .map((orders) =>
        runningEnergyTrace(orders, beta, run.energyShift, run.numSites),
      );
    renderConvergence(el("convergence"), traces, combined);
  }

  const elapsed = (performance.now() - run.startedAt) / 1000;
  const rate = run.sweepsDone / Math.max(elapsed, 0.001);
  const totalSweeps = run.totalSweepsPerChain * run.config.execution.chains;
  const eta = Math.max(0, (totalSweeps - run.sweepsDone) / Math.max(rate, 1));
  el("status-line").textContent =
    `${label} · ${Math.round(rate).toLocaleString()} sweeps/s · ` +
    `${run.completed.size}/${run.config.execution.chains} chains · ` +
    `~${formatDuration(eta)} remaining`;
}

function formatEnergy(combined) {
  if (combined.standardError === null) return combined.mean.toFixed(6);
  return `${combined.mean.toFixed(6)} ± ${combined.standardError.toExponential(1)}`;
}

function formatDuration(seconds) {
  if (seconds < 90) return `${Math.ceil(seconds)} s`;
  return `${Math.ceil(seconds / 60)} min`;
}

// ---------------------------------------------------------------- campaigns

async function runCampaign() {
  const ui = readForm();
  if (isLocalOnly(ui)) {
    el("yaml").focus();
    el("yaml").scrollIntoView({ behavior: "smooth", block: "center" });
    return;
  }
  if (activeCampaign) return;

  el("results-idle").classList.add("hidden");
  el("results-live").classList.remove("hidden");
  el("cancel").classList.remove("hidden");
  el("share").classList.add("hidden");
  el("ladder").classList.add("hidden");
  el("chain-table").classList.add("hidden");
  el("badges").replaceChildren();
  runButton.disabled = true;

  const betas = ui.betaAuto ? ladderBetas(ui) : [ui.beta];
  const ladderPoints = [];
  let lastRun = null;
  const campaign = { cancelled: false, currentRun: null };
  activeCampaign = campaign;
  el("cancel").onclick = () => {
    campaign.cancelled = true;
    campaign.currentRun?.cancel?.();
  };

  for (let i = 0; i < betas.length && !campaign.cancelled; i += 1) {
    const isFinal = i === betas.length - 1;
    const sweeps = isFinal ? ui.sweeps : Math.max(500, Math.round(ui.sweeps / 4));
    const config = buildRunConfig(ui, betas[i], sweeps);
    const label = ui.betaAuto
      ? `β ladder ${i + 1}/${betas.length} (β=${betas[i]})`
      : `sampling at β=${betas[i]}`;
    const { promise, cancel } = runOnce(config, label, scheduleRender);
    campaign.currentRun = { cancel };
    const run = await promise;
    if (run.cancelled || run.error) {
      finishCampaign(null, null, ui, run.error);
      return;
    }
    const beta = config.simulation.beta;
    const energies = run.orders.map(
      (orders) =>
        chainThermodynamics(orders, beta, run.energyShift, run.numSites).energyPerSite,
    );
    ladderPoints.push({ beta, ...combine(energies) });
    lastRun = run;
  }
  finishCampaign(lastRun, ladderPoints, ui, null);
}

function finishCampaign(run, ladderPoints, ui, error) {
  activeCampaign = null;
  runButton.disabled = false;
  el("cancel").classList.add("hidden");
  if (!run) {
    el("status-line").textContent = error
      ? `failed: ${error}`
      : "cancelled";
    return;
  }
  const config = run.config;
  const beta = config.simulation.beta;
  const final = ladderPoints[ladderPoints.length - 1];
  el("headline-value").textContent = formatEnergy({
    mean: final.mean,
    standardError: final.standardError,
  });
  el("headline-label").textContent = ui.betaAuto
    ? `E/site — ground-state estimate (β = ${beta})`
    : `E/site — thermal energy at β = ${beta}`;

  // Quality badges from the final run.
  const rHat = splitRHat(run.orders);
  const perChain = run.orders.map((orders) => {
    const acf = autocorrelation(orders);
    const thermo = chainThermodynamics(orders, beta, run.energyShift, run.numSites);
    return { acf, thermo };
  });
  const minEss = Math.min(...perChain.map(({ acf }) => acf.ess));
  const badges = el("badges");
  badges.replaceChildren(
    badge(
      rHat === null ? "R̂ n/a" : `R̂ ${rHat.toFixed(3)}`,
      rHat !== null && rHat < 1.01,
      "Split-chain potential scale reduction; below 1.01 indicates converged chains.",
    ),
    badge(
      `ESS ≥ ${Math.round(minEss).toLocaleString()}`,
      minEss >= 100,
      "Smallest autocorrelation-adjusted effective sample size across chains.",
    ),
    badge(
      `C/site ${combine(perChain.map(({ thermo }) => thermo.heatCapacityPerSite)).mean.toFixed(4)}`,
      true,
      "Heat-capacity estimator from expansion-order fluctuations.",
    ),
  );

  // Chain table.
  const tbody = el("chain-table").querySelector("tbody");
  tbody.replaceChildren(
    ...perChain.map(({ acf, thermo }, index) => {
      const row = document.createElement("tr");
      row.innerHTML =
        `<td>${index}</td><td>${thermo.energyPerSite.toFixed(6)}</td>` +
        `<td>${Math.round(acf.ess).toLocaleString()}</td><td>${acf.tau.toFixed(2)}</td>`;
      return row;
    }),
  );
  el("chain-table").classList.remove("hidden");

  if (ladderPoints.length > 1) {
    el("ladder").classList.remove("hidden");
    renderLadder(el("ladder"), ladderPoints.map((point) => ({
      beta: point.beta,
      energyPerSite: point.mean,
      standardError: point.standardError,
    })));
  }

  const elapsed = (performance.now() - run.startedAt) / 1000;
  el("status-line").textContent =
    `done in ${formatDuration(elapsed)} · ${config.execution.chains} chains × ` +
    `${config.simulation.measurement_sweeps.toLocaleString()} measurements · seed ${config.execution.seed}`;

  // Sharing + history.
  const encoded = encodeConfig(ui);
  window.location.hash = `c=${encoded}`;
  el("share").classList.remove("hidden");
  el("share").onclick = () => copyText(window.location.href, el("share"));
  pushHistory(ui, final);
}

function badge(text, ok, title) {
  const node = document.createElement("span");
  node.className = `badge ${ok ? "badge-ok" : "badge-warn"}`;
  node.textContent = text;
  node.title = title;
  return node;
}

// ------------------------------------------------------------------ history

function pushHistory(ui, final) {
  const entries = loadHistory();
  entries.unshift({
    when: new Date().toISOString(),
    ui,
    headline: { mean: final.mean, standardError: final.standardError },
  });
  localStorage.setItem("sse-web-history", JSON.stringify(entries.slice(0, 8)));
  renderHistory();
}

function loadHistory() {
  try {
    return JSON.parse(localStorage.getItem("sse-web-history") ?? "[]");
  } catch {
    return [];
  }
}

function renderHistory() {
  const entries = loadHistory();
  const rail = el("history");
  if (!entries.length) {
    rail.innerHTML = '<p class="hint">No runs yet.</p>';
    return;
  }
  rail.replaceChildren(
    ...entries.map((entry) => {
      const item = document.createElement("button");
      item.className = "history-item";
      const label =
        entry.ui.ly === 1
          ? `chain ${entry.ui.lx}`
          : `${entry.ui.lx}×${entry.ui.ly}`;
      item.innerHTML =
        `<b>${entry.headline.mean.toFixed(5)}</b>` +
        `<span>${entry.ui.model} · ${label} · ${new Date(entry.when).toLocaleTimeString()}</span>`;
      item.onclick = () => {
        applyUi(entry.ui);
        syncAll();
      };
      return item;
    }),
  );
}

// -------------------------------------------------------------- url sharing

function encodeConfig(ui) {
  return btoa(JSON.stringify(ui)).replaceAll("+", "-").replaceAll("/", "_");
}

function decodeConfig(encoded) {
  try {
    return JSON.parse(atob(encoded.replaceAll("-", "+").replaceAll("_", "/")));
  } catch {
    return null;
  }
}

function applyUi(ui) {
  setModel(ui.model);
  el("lx").value = ui.lx;
  el("ly").value = ui.ly;
  el("periodic-x").checked = ui.periodicX;
  el("periodic-y").checked = ui.periodicY;
  el("tfim-j").value = ui.j;
  el("tfim-h").value = ui.h;
  el("ryd-omega").value = ui.omega;
  el("ryd-detuning").value = ui.detuning;
  el("ryd-c6").value = ui.c6;
  el("beta-auto").checked = ui.betaAuto;
  el("beta").value = ui.beta;
  el("chains").value = ui.chains;
  el("sweeps").value = ui.sweeps;
  el("seed").value = ui.seed;
}

// -------------------------------------------------------------------- wiring

function setModel(model) {
  el("model-tfim").classList.toggle("selected", model === "tfim");
  el("model-tfim").setAttribute("aria-checked", String(model === "tfim"));
  el("model-rydberg").classList.toggle("selected", model === "rydberg");
  el("model-rydberg").setAttribute("aria-checked", String(model === "rydberg"));
}

async function copyText(text, button) {
  try {
    await navigator.clipboard.writeText(text);
    const original = button.textContent;
    button.textContent = "copied ✓";
    setTimeout(() => {
      button.textContent = original;
    }, 1200);
  } catch {
    window.prompt("Copy:", text);
  }
}

function wire() {
  el("model-tfim").onclick = () => {
    setModel("tfim");
    syncAll();
  };
  el("model-rydberg").onclick = () => {
    setModel("rydberg");
    syncAll();
  };
  for (const chip of el("size-chips").querySelectorAll(".chip")) {
    chip.onclick = () => {
      const [lx, ly] = chip.dataset.size.split(",").map(Number);
      el("lx").value = lx;
      el("ly").value = ly;
      for (const other of el("size-chips").querySelectorAll(".chip")) {
        other.classList.toggle("selected", other === chip);
      }
      syncAll();
    };
  }
  for (const id of [
    "lx", "ly", "periodic-x", "periodic-y", "tfim-j", "tfim-h",
    "ryd-omega", "ryd-detuning", "ryd-c6", "beta-auto", "beta",
    "chains", "sweeps", "seed",
  ]) {
    el(id).addEventListener("input", () => {
      for (const chip of el("size-chips").querySelectorAll(".chip")) {
        chip.classList.remove("selected");
      }
      syncAll();
    });
  }
  el("reseed").onclick = () => {
    el("seed").value = Math.floor(Math.random() * 1_000_000);
    syncAll();
  };
  runButton.onclick = runCampaign;
  el("copy-yaml").onclick = () => copyText(el("yaml").textContent, el("copy-yaml"));
  el("copy-command").onclick = () =>
    copyText(
      "sse run --config run.yaml --output results/run-01",
      el("copy-command"),
    );
  el("theme-toggle").onclick = () => {
    const order = ["", "light", "dark"];
    const current = document.documentElement.dataset.theme ?? "";
    const next = order[(order.indexOf(current) + 1) % order.length];
    if (next) document.documentElement.dataset.theme = next;
    else delete document.documentElement.dataset.theme;
    localStorage.setItem("sse-web-theme", next);
  };
  window.addEventListener("resize", () => syncAll());
}

async function boot() {
  const theme = localStorage.getItem("sse-web-theme");
  if (theme) document.documentElement.dataset.theme = theme;
  wire();
  const shared = new URLSearchParams(window.location.hash.slice(1)).get("c");
  if (shared) {
    const ui = decodeConfig(shared);
    if (ui) applyUi(ui);
  }
  renderHistory();
  syncAll();
  try {
    await init();
    // Cheap self-check so a broken module disables the run button early.
    validate_config(
      JSON.stringify(buildRunConfig(readForm(), 4, 100)),
    );
    wasmReady = true;
    el("version-line").textContent =
      `sse-web ${APP_VERSION} · qslib-quantum 0.2.0 · seed scheme qslib-seed-v1 · wasm ready`;
  } catch (error) {
    runButton.disabled = true;
    el("run-hint").textContent = `wasm module failed to load: ${error.message ?? error}`;
    return;
  }
  try {
    const response = await fetch(new URL("./presets.json", import.meta.url));
    if (response.ok) {
      const data = await response.json();
      presets = new Map(Object.entries(data.references ?? {}));
    }
  } catch {
    // Presets are an enhancement; their absence is not an error.
  }
  syncAll();
}

boot();
