/**
 * Each browser Worker runs an independent instance of Slopjet's Rust/WASM scanner.
 * Only request-level parallelism; no shared state or parser fallback.
 */
let initialization = Promise.reject(new Error("worker not initialized"));
initialization.catch(() => {});
let Session = null;

async function initialize(glueUrl, wasmUrl) {
  if (!glueUrl || !wasmUrl) throw new Error("WASM glue and binary URLs required");
  const exports = await import(glueUrl);
  if (typeof exports.PerftestRewriteSession !== "function") {
    throw new Error("This WASM build has no Rust scanner export");
  }
  const response = await fetch(wasmUrl);
  if (!response.ok) throw new Error("Unable to fetch WASM: " + response.status);
  exports.initSync({ module: new WebAssembly.Module(await response.arrayBuffer()) });
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
      options.disableComputedWrap,
      options.importfn, options.rewritefn, options.postmessagefn, options.base
    );
    try {
      const decoder = new TextDecoder("utf-8", { fatal: true });
      for (let i = 0; i < source.byteLength; i += 16384) {
        scanner.write(decoder.decode(source.subarray(i, i + 16384), { stream: true }));
      }
      scanner.write(decoder.decode());
      const result = scanner.finish();
      if (typeof result !== "string") {
        throw new SyntaxError("Unsupported syntax in Slopjet standalone scanner");
      }
      self.postMessage({ id, ok: true, result });
    } finally {
      scanner.free?.();
    }
  } catch (error) {
    self.postMessage({ id, ok: false, error: String(error?.message ?? error) });
  }
});
