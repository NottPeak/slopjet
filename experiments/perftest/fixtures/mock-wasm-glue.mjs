export function initSync() { return {}; }
export class PerftestRewriteSession {
  constructor() { this.buffer = ""; }
  write(fragment) { this.buffer += fragment; }
  finish() { return this.buffer === "location;" ? "wrap(location);" : undefined; }
  free() {}
}
