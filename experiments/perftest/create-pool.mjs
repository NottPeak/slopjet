import { RequestPool } from "./browser-pool.mjs";

/**
 * Create a page-hosted pool for the opt-in ControllerInit.perftestWorkers API.
 *
 * Example:
 * const pool = createPerftestPool({
 *   wasmGlueUrl: "/your-assets/wasm.js",
 *   wasmUrl: "/scramjet/scramjet.wasm",
 * });
 * const controller = new Controller({
 *   serviceworker, transport, perftestWorkers: pool,
 * });
 *
 * Host wasm-worker.mjs and its glue JS on the SAME origin as the page.
 * Keep the configured pool alive while the controller is serving requests.
 */
export function createPerftestPool({
  workers = Math.max(1, Math.floor((globalThis.navigator?.hardwareConcurrency ?? 2) / 2)),
  maxQueue = 1024,
  workerUrl = new URL("./wasm-worker.mjs", import.meta.url).href,
  wasmGlueUrl,
  wasmUrl,
} = {}) {
  if (typeof Worker !== "function") {
    throw new Error("Dedicated Workers unavailable; run in a window context, not a Service Worker");
  }
  if (typeof wasmGlueUrl !== "string" || !wasmGlueUrl) {
    throw new TypeError("wasmGlueUrl is required");
  }
  if (typeof wasmUrl !== "string" || !wasmUrl) {
    throw new TypeError("wasmUrl is required");
  }
  const glueUrl = new URL(wasmGlueUrl, globalThis.location.href).href;
  const binaryUrl = new URL(wasmUrl, globalThis.location.href).href;
  return new RequestPool({
    workers, maxQueue,
    workerFactory() {
      const worker = new Worker(workerUrl, { type: "module" });
      worker.postMessage({ type: "init", glueUrl, wasmUrl: binaryUrl });
      return worker;
    },
  });
}
