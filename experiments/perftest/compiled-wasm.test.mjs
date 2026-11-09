import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const wasm = await import("../../packages/core/rewriter/wasm/out/wasm.js");
const bytes = await readFile(
  new URL("../../packages/core/rewriter/wasm/out/wasm_bg.wasm", import.meta.url)
);
wasm.initSync({ module: new WebAssembly.Module(bytes) });

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
  sourcemaps: false, captureErrors: false, scramitize: false,
  disableComputedWrap: false, destructureRewrites: false,
};

function rewrite(source) {
  const engine = new wasm.Rewriter();
  try {
    const output = engine.rewrite_js(
      cfg, flags, () => { throw new Error("URL codec must not be invoked"); },
      source, "https://example.com/", "test.js", false
    );
    assert.ok(output.js instanceof Uint8Array);
    return new TextDecoder().decode(output.js);
  } finally {
    engine.free();
  }
}
function stream(source, n = 1) {
  const engine = new wasm.PerftestRewriteSession(
    cfg.wrapfn, cfg.wrappropertybase, cfg.wrappropertyfn, false,
    cfg.importfn, cfg.rewritefn, cfg.wrappostmessagefn, "https://example.com/",
    cfg.metafn, false
  );
  try {
    for (let i = 0; i < source.length; i += n) {
      engine.write(source.slice(i, i + n));
    }
    return engine.finish();
  } finally {
    engine.free();
  }
}

test("the compiled standalone WASM engine rewrites ordinary JS expressions", () => {
  const cases = [
    ["const x = window.location.href;", "const x = window.$property$location.href;"],
    ["function f(x){return x.location + top.location;}",
      "function f(x){return x.$property$location + $wrap(top).$property$location;}"],
    ["let x = obj[location];", "let x = obj[$wrapProperty(($wrap(location)))];"],
    ["const x = /location/gi; // location", "const x = /location/gi; // location"],
    ["let x = 10 / 2;", "let x = 10 / 2;"],
    ["const m = import('./x.js');", "const m = $import(\"https://example.com/\",'./x.js');"],
    ["eval('window.location');", "eval($rewrite('window.location'));"],
    ["window.postMessage('x');", "$postMessage(window).postMessage('x');"],
  ];
  for (const [source, expected] of cases) {
    assert.equal(rewrite(source), expected, source);
    for (let size = 1; size <= Math.min(12, source.length); size++) {
      assert.equal(stream(source, size), expected, source + ": chunk " + size);
    }
  }
});

test("unsupported syntax is rejected, not passed through", () => {
  const invalid = [
    "const location = 5;",
    "function f(location){return location}",
    "location = '/path';",
    "import x from 'lib';",
    "eval(location, top)",
    "obj.foo.postMessage('hello')",
  ];
  for (const source of invalid) {
    assert.notEqual(typeof stream(source), "string", source);
    assert.throws(() => rewrite(source), /Unsupported JavaScript syntax/);
  }
});

test("unsupported flags fail explicitly", () => {
  const engine = new wasm.Rewriter();
  try {
    assert.throws(
      () => engine.rewrite_js(cfg, { ...flags, sourcemaps: true }, () => "",
        "top;", "https://example.com/", "t.js", false),
      /not yet implemented/
    );
  } finally {
    engine.free();
  }
});
