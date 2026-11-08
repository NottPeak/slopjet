/**
 * Dedicated-worker implementation for Slopjet's perftest Rust/WASM fast path.
 * Must be hosted by a page or a dedicated/shared worker, NOT a service worker.
 *
 * Initialization message: {type:"init", glueUrl, wasmUrl}
 * Request message: {id, payload:{source:Uint8Array, options:{...}}}
 * Response message: {id, ok, result?:string|null, error?:string}
 */
let initialization = Promise.reject(new Error("worker not initialized"));
initialization.catch(() => {}); // suppress rejection until a task awaits init
let Session = null;

async function initialize(glueUrl, wasmUrl) {
  if (!glueUrl || !wasmUrl) throw new Error("glueUrl and wasmUrl are required");
  const exports = await import(glueUrl);
  if (typeof exports.PerftestRewriteSession !== "function") {
    throw new Error("WASM not compiled with perftest feature");
  }
  const response = await fetch(wasmUrl);
  if (!response.ok) throw new Error("Unable to fetch WASM: " + response.status);
  const bytes = await response.arrayBuffer();
  exports.initSync({ module: new WebAssembly.Module(bytes) });
  Session = exports.PerftestRewriteSession;
}

self.addEventListener("message", async (event) => {
  const data = event.data;
  if (data?.type === "init") {
    initialization = initialize(data.glueUrl, data.wasmUrl);
    initialization.catch(() => {});
    return;
  }
  const id = data?.id;
  if (!Number.isSafeInteger(id)) return;
  try {
    await initialization;
    const { source, options } = data.payload ?? {};
    if (!(source instanceof Uint8Array)) throw new TypeError("source must be Uint8Array");
    if (!options) throw new TypeError("missing rewrite options");
    const scanner = new Session(
      options.wrapfn, options.propertybase, options.propertyfn,
      options.disableComputedWrap
    );
    try {
      const decoder = new TextDecoder("utf-8");
      // Chunked feed uses the same state machine as network streaming.
      for (let i = 0; i < source.byteLength; i += 16384) {
        scanner.write(decoder.decode(source.subarray(i, i + 16384), { stream: true }));
      }
      scanner.write(decoder.decode());
      self.postMessage({ id, ok: true, result: scanner.finish() });
    } finally {
      scanner.free?.();
    }
  } catch (error) {
    self.postMessage({ id, ok: false, error: String(error?.message ?? error) });
  }
});
