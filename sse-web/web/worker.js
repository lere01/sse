// One Web Worker owns one independent Monte Carlo chain.
//
// The main thread drives the loop ("ticks") so it can pause, cancel, and
// adapt batch sizes; the worker only creates the chain and advances it.
// Each worker holds its own wasm instance - no shared memory, no special
// cross-origin headers, works on plain GitHub Pages.

import init, { ChainHandle } from "./pkg/sse_web.js";

let handle = null;
let chainIndex = null;
let ready = false;

self.onmessage = async (event) => {
  const message = event.data;
  try {
    if (message.type === "init") {
      await init({ module_or_path: message.wasmUrl });
      ready = true;
      self.postMessage({ type: "ready" });
      return;
    }
    if (message.type === "create") {
      if (!ready) throw new Error("worker received create before init");
      chainIndex = message.chainIndex;
      handle = new ChainHandle(JSON.stringify(message.config), chainIndex);
      self.postMessage({ type: "created", chainIndex });
      return;
    }
    if (message.type === "tick") {
      if (!handle) throw new Error("worker received tick before create");
      const started = performance.now();
      const report = JSON.parse(handle.advance(message.sweeps));
      self.postMessage({
        type: "report",
        chainIndex,
        sweeps: message.sweeps,
        elapsedMs: performance.now() - started,
        report,
      });
      return;
    }
    if (message.type === "dispose") {
      if (handle) handle.free();
      handle = null;
      self.postMessage({ type: "disposed", chainIndex });
    }
  } catch (error) {
    self.postMessage({
      type: "error",
      chainIndex,
      message: String(error?.message ?? error),
    });
  }
};
