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
let FullRewriter = null;
let rewriterInstance = null;

async function initialize(glueUrl, wasmUrl) {
  if (!glueUrl || !wasmUrl) throw new Error("glueUrl and wasmUrl are required");
  const exports = await import(glueUrl);
  if (typeof exports.PerftestRewriteSession !== "function" ||
      typeof exports.Rewriter !== "function") {
    throw new Error("WASM missing required perftest/Rewriter exports");
  }
  const response = await fetch(wasmUrl);
  if (!response.ok) throw new Error("Unable to fetch WASM: " + response.status);
  const bytes = await response.arrayBuffer();
  exports.initSync({ module: new WebAssembly.Module(bytes) });
  Session = exports.PerftestRewriteSession;
  FullRewriter = exports.Rewriter;
  rewriterInstance = new FullRewriter();
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
    const { source, options, mode = "fast" } = data.payload ?? {};
    if (!(source instanceof Uint8Array)) throw new TypeError("source must be Uint8Array");
    if (!options) throw new TypeError("missing rewrite options");
    if (mode === "full") {
      // Each dedicated Worker owns its own reusable OXC/WASM instance.
      // The WASM wrapper resets its allocator after every rewrite.
      const result = rewriterInstance.rewrite_js_bytes(
        options.jsconfig,
        options.flags,
        (url) => encodeURIComponent(url),
        source,
        options.base,
        options.url,
        options.isModule
      );
      const bytes = result.js.slice();
      self.postMessage({ id, ok: true, result: bytes }, [bytes.buffer]);
      return;
    }
    if (mode !== "fast") throw new TypeError("Unknown rewrite mode");
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
