// Measure compiled Rust/WASM rewriting across independent Node worker_threads.
// Warm worker startup and compilation are excluded from request throughput;
// do not equate these results to browser or network end-to-end performance.
import { Worker } from "node:worker_threads";
import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";

const count = Number.parseInt(process.argv[2] ?? "120", 10);
if (!Number.isSafeInteger(count) || count < 1) {
  throw new RangeError("request count must be positive");
}
const source = (
  "const url = window.location.href;\n" +
  "function f(top){return top + parent.location;}\n" +
  "const value = obj[location];\n" +
  "const x = parent.top.location;\n"
).repeat(180);
const bytes = Buffer.byteLength(source) * count;
let expected = null;

async function benchmark(workers) {
  const pending = new Map();
  const slots = [];
  const initialized = [];
  for (let index = 0; index < workers; index++) {
    const worker = new Worker(new URL("./node-wasm-worker.mjs", import.meta.url), {
      type: "module",
    });
    let resolveReady;
    let rejectReady;
    const ready = new Promise((resolve, reject) => {
      resolveReady = resolve;
      rejectReady = reject;
    });
    initialized.push(ready);
    worker.on("message", (message) => {
      if (message.ready) { resolveReady(); return; }
      const entry = pending.get(message.id);
      if (!entry) return;
      pending.delete(message.id);
      if (message.error) entry.reject(new Error(message.error));
      else entry.resolve(message.result);
    });
    const fail = (error) => {
      rejectReady(error);
      for (const task of pending.values()) task.reject(error);
      pending.clear();
    };
    worker.on("error", fail);
    worker.on("exit", (code) => {
      if (code !== 0) fail(new Error("worker exited: " + code));
    });
    slots.push(worker);
  }
  try {
    await Promise.all(initialized);
    const send = (id, worker) => new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      worker.postMessage({ id, source });
    });
    // Warm each instance so these numbers measure steady-state requests.
    await Promise.all(slots.map((worker, id) => send(-(id + 1), worker)));
    const start = performance.now();
    const results = await Promise.all(
      Array.from({ length: count }, (_, id) => send(id, slots[id % slots.length]))
    );
    const elapsedMs = performance.now() - start;
    if (expected === null) expected = results[0];
    assert.ok(expected.includes("$property$location"), "member rewrite absent");
    assert.ok(expected.includes("$wrap(parent)"), "unsafe global rewrite absent");
    for (const value of results) assert.equal(value, expected);
    const throughput = bytes / 1048576 / (elapsedMs / 1000);
    console.log(
      "compiled-wasm workers=" + workers + " requests=" + count +
      " bytes=" + bytes + " elapsed=" + elapsedMs.toFixed(2) +
      "ms MiB/s=" + throughput.toFixed(2)
    );
  } finally {
    await Promise.all(slots.map((worker) => worker.terminate()));
  }
}

for (const workers of [1, 2, 4, 8]) {
  await benchmark(workers);
}
