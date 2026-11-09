// Native Node worker_threads benchmark adapter for the *compiled* Rust/WASM
// rewriter. Each worker owns its WASM instance and a new scanner per request.
import { parentPort } from "node:worker_threads";
import { readFileSync } from "node:fs";
import * as wasm from "../../packages/core/rewriter/wasm/out/wasm.js";

const binary = readFileSync(new URL("../../packages/core/rewriter/wasm/out/wasm_bg.wasm", import.meta.url));
wasm.initSync({ module: new WebAssembly.Module(binary) });

parentPort.on("message", ({ id, source }) => {
  let scanner;
  try {
    scanner = new wasm.PerftestRewriteSession(
      "$wrap", "$property$", "$wrapProperty", false,
      "$import", "$rewrite", "$postMessage",
      "https://example.test/script.js", "$meta", false, "$tryset"
    );
    for (let offset = 0; offset < source.length; offset += 4096) {
      scanner.write(source.slice(offset, offset + 4096));
    }
    const result = scanner.finish();
    if (typeof result !== "string") throw new Error("unsupported benchmark fixture");
    parentPort.postMessage({ id, result });
  } catch (error) {
    parentPort.postMessage({ id, error: String(error) });
  } finally {
    scanner?.free();
  }
});

parentPort.postMessage({ ready: true });
