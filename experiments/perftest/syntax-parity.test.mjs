import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFile } from "node:fs/promises";

const glue = await import("../../packages/core/rewriter/wasm/out/wasm.js");
const bytes = await readFile(new URL(
  "../../packages/core/rewriter/wasm/out/wasm_bg.wasm", import.meta.url
));
glue.initSync({ module: new WebAssembly.Module(bytes) });
const config = {
  prefix: "/scramjet/", wrapfn: "$wrap",
  wrappropertybase: "$property$", wrappropertyfn: "$computed",
  cleanrestfn: "$clean", importfn: "$import", rewritefn: "$rewrite",
  wrappostmessagefn: "$post", metafn: "$meta",
  pushsourcemapfn: "$pushmap", trysetfn: "$tryset",
  templocid: "$tempLoc", tempunusedid: "$tempUnused",
};
const flags = {
  sourcemaps: false, captureErrors: false, scramitize: false,
  disableComputedWrap: false, destructureRewrites: false,
};
function transform(input) {
  const rewriter = new glue.Rewriter();
  try {
    const output = rewriter.rewrite_js(
      config, flags, () => { throw Error("unexpected URL rewrite"); },
      input, "https://proxy.test/app.js", "app.js", false
    );
    return new TextDecoder().decode(output.js);
  } finally {
    rewriter.free();
  }
}
function run(source) {
  const context = {
    result: null,
    $wrap: () => { throw new Error("shadowed global was rewritten"); },
    $computed: value => value,
    $rewrite: value => value,
    $post: value => value,
    $tryset: () => false,
  };
  const script = new vm.Script(source);
  script.runInNewContext(context, { timeout: 1000 });
  return context.result;
}

test("rewrites retain valid JavaScript grammar across common syntax", () => {
  const cases = [
    "const x=1; let y=2; x+y;",
    "const values=[1,2,3]; values.map(x=>x*2);",
    "const o={a:1,b:2}; ({a:o.a,b:o.b});",
    "function sum(a,b=2){return a+b} sum(1);",
    "const hello = (name)=>'hello '+name;",
    "const z = /ab+c/gi.test('abbbc');",
    "class A { constructor(x){this.x=x;} get(){return this.x;} }",
    "let sum=0; for(let i=0;i<5;i++){sum+=i;}",
    "let a=1; try {a++} catch(e){a--}",
    "const o={a:1}; const {a}=o;",
    "const o={a:1}; const copy={...o};",
    "function f(top){return top+1;} f(4);",
    "function f(){ const top=4; return top+1; } f();",
    "const a = new Map(); a.set('x',1); a.get('x');",
    "const x = 12 / 4; const y = x+7;",
  ];
  let supported = 0;
  for (const source of cases) {
    let result;
    try { result = transform(source); } catch { continue; }
    supported++;
    assert.doesNotThrow(() => new vm.Script(result), source + " -> " + result);
  }
  assert.ok(supported >= 10, "low supported-language coverage: " + supported);
});

test("local protected identifiers are not rewritten", () => {
  const cases = [
    ["function f(top){ return top+1; } result=f(4);", 5],
    ["const top=3; result=top+2;", 5],
    ["let top=9; { let top=2; result=top+3; }", 5],
    ["function f(){ var top=4; return top+1; } result=f();", 5],
  ];
  for (const [source, expected] of cases) {
    const rewritten = transform(source);
    assert.equal(run(rewritten), expected, source + " -> " + rewritten);
  }
});

test("known constructor escape paths never emit unprotected JS", () => {
  for (const source of [
    "new top.Function('return this')();",
    "parent.constructor.constructor('return this')();",
    "globalThis.__proto__.constructor('return this')();",
    "window['Function']('return this')();",
  ]) {
    assert.throws(() => transform(source), /Unsupported JavaScript syntax/, source);
  }
});
