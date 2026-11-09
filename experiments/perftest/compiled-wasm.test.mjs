import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const wasmModule = await import("../../packages/core/rewriter/wasm/out/wasm.js");
const binary = await readFile(
  new URL("../../packages/core/rewriter/wasm/out/wasm_bg.wasm", import.meta.url)
);
const { initSync, PerftestRewriteSession, Rewriter } = wasmModule;
initSync({ module: new WebAssembly.Module(binary) });

const cfg = {
  prefix: "/scramjet/",
  wrapfn: "$wrap",
  wrappropertybase: "$property$",
  wrappropertyfn: "$wrapProperty",
  cleanrestfn: "$cleanRest",
  importfn: "$import",
  rewritefn: "$rewrite",
  wrappostmessagefn: "$postMessage",
  metafn: "$meta",
  pushsourcemapfn: "$pushmap",
  trysetfn: "$tryset",
  templocid: "$tempLoc",
  tempunusedid: "$tempUnused",
};
const flags = {
  sourcemaps: false,
  captureErrors: false,
  scramitize: false,
  disableComputedWrap: false,
  destructureRewrites: false,
};

function scan(source, size = source.length) {
  const scanner = new PerftestRewriteSession(
    cfg.wrapfn, cfg.wrappropertybase, cfg.wrappropertyfn, false
  );
  try {
    for (let i = 0; i < source.length; i += size) {
      scanner.write(source.slice(i, i + size));
    }
    return scanner.finish();
  } finally {
    scanner.free();
  }
}

function legacy(source) {
  const rewriter = new Rewriter();
  try {
    const out = rewriter.rewrite_js(
      cfg, flags, (x) => encodeURIComponent(x),
      source, "https://example.com/", "test.js", false
    );
    return new TextDecoder().decode(out.js);
  } finally {
    rewriter.free();
  }
}

test("compiled WASM fast path matches OXC for supported programs", () => {
  for (const source of [
    "top;", "window.location;", "this.location;",
    "'use strict'; top.location; window.location;",
    "window[location];", "parent['top'];", "obj[foo];",
  ]) {
    const expected = legacy(source);
    assert.equal(scan(source), expected, source);
    for (let n = 1; n < Math.min(source.length, 12); n++) {
      assert.equal(scan(source, n), expected, source + " chunk=" + n);
    }
  }
});

test("compiled WASM declines unsupported syntax with no output", () => {
  for (const source of [
    "const x = 1;",
    "window.location; let window = customWindow;",
    "const r = /foo/;",
    "import('./x.js');",
    "window?.location;",
    "function f(x) { return x; }",
  ]) {
    assert.notEqual(typeof scan(source), "string", source);
    assert.ok(legacy(source).length > 0, source);
  }
});
