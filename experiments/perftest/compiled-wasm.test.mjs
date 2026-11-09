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

function rewrite(source, module = false) {
  const engine = new wasm.Rewriter();
  try {
    const output = engine.rewrite_js(
      cfg, flags, (specifier) => "/proxy/" + specifier,
      source, "https://example.com/", "test.js", module
    );
    assert.ok(output.js instanceof Uint8Array);
    return new TextDecoder().decode(output.js);
  } finally {
    engine.free();
  }
}
function stream(source, n = 1, module = false) {
  const engine = new wasm.PerftestRewriteSession(
    cfg.wrapfn, cfg.wrappropertybase, cfg.wrappropertyfn, false,
    cfg.importfn, cfg.rewritefn, cfg.wrappostmessagefn, "https://example.com/",
    cfg.metafn, module, cfg.trysetfn
  );
  try {
    for (let i = 0; i < source.length; i += n) {
      engine.write(source.slice(i, i + n));
    }
    return engine.finish((specifier) => "/proxy/" + specifier);
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
    ["eval(location, top)", "eval($rewrite($wrap(location)), $wrap(top))"],
    ["window.postMessage('x');", "$postMessage(window).postMessage('x');"],
    ["location = top;", '((t)=>$tryset(location,"=",t)||(location=t))(  $wrap(top));'],
  ];
  for (const [source, expected] of cases) {
    assert.equal(rewrite(source), expected, source);
    for (let size = 1; size <= Math.min(12, source.length); size++) {
      assert.equal(stream(source, size), expected, source + ": chunk " + size);
    }
  }
});

test("module metadata and named exports rewrite through compiled WASM", () => {
  const cases = [
    ["const url = import.meta.url;", 'const url = $meta(import.meta,"https://example.com/").url;'],
    ["export const url = top.location;", "export const url = $wrap(top).$property$location;"],
  ];
  for (const [src, expected] of cases) {
    assert.equal(rewrite(src, true), expected);
    for (let n = 1; n <= src.length; n++) {
      assert.equal(stream(src, n, true), expected);
    }
  }
});
test("compiled WASM resolves shadowed globals in block and function scope", () => {
  const cases = [
    ["const location = 5; location;", "const location = 5; location;"],
    ["function f(location){return location}", "function f(location){return location}"],
    ["top; { let top = 3; top; } top;", "$wrap(top); { let top = 3; top; } $wrap(top);"],
  ];
  for (const [source, expected] of cases) {
    assert.equal(rewrite(source), expected);
    for (let n = 1; n < Math.min(source.length, 10); n++) {
      assert.equal(stream(source, n), expected);
    }
  }
});

test("static import and re-export URLs use the host resolver", () => {
  const source = "import x from './mod.js'; export {x} from '../other.mjs';";
  const expected = 'import x from "/proxy/./mod.js"; export {x} from "/proxy/../other.mjs";';
  assert.equal(rewrite(source, true), expected);
  for (let size = 1; size < 12; size++) {
    assert.equal(stream(source, size, true), expected);
  }
});

test("object and array destructuring declarations preserve semantics", () => {
  const scripts = [
    "const {a,b} = obj;",
    "const {a:renamed, ...rest} = obj;",
    "const {a='fallback'} = obj;",
    "const [a,b=1,...rest] = values;",
    "const {[key]: value} = obj;",
  ];
  for (const source of scripts) {
    assert.equal(rewrite(source), source, source);
    for (let size = 1; size < 7; size++) {
      assert.equal(stream(source, size), source, source + " chunk=" + size);
    }
  }
});

test("unsupported syntax is rejected, not passed through", () => {
  const invalid = [
    "const {location} = window;",
    "function f({location}){return location}",
    "location += '/path';",
    "import x from 'lib';",
    "eval(...args)",
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
