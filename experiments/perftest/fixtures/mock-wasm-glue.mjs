export function initSync() { return {}; }
export class Rewriter {
  rewrite_js_bytes(config, flags, encode, source, base, url, isModule) {
    if (encode("https://x/y?z=1") !== "https%3A%2F%2Fx%2Fy%3Fz%3D1") {
      throw new Error("wrong URL codec");
    }
    return {js:new Uint8Array(source), map:new Uint8Array([0,0,0,0]), scramtag:"test", errors:[]};
  }
  free() {}
}
export class PerftestRewriteSession {
  constructor() { this.buffer = ""; }
  write(fragment) { this.buffer += fragment; }
  finish() { return this.buffer === "location;" ? "wrap(location);" : null; }
  free() {}
}
