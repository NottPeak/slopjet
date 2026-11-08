// Cases intended for a future differential harness against current Scramjet.
export const cases = [
  { name: "global-member", source: 'window.location.href = "/dashboard";' },
  { name: "shadowed-parameter", source: 'function f(window) { return window.location; }' },
  { name: "later-lexical-binding", source: 'console.log(window.location); let window = customWindow;' },
  { name: "string-and-regexp", source: 'const a = "window.location"; const b = /window\\.location/;' },
  { name: "template-interpolation", source: 'const x = `hello ${window.location.hostname}`;' },
  { name: "destructure", source: 'const { location: loc } = window; console.log(loc);' },
  { name: "nested-regexp", source: 'const r = /[/*]x/; /* comment */ fetch("/a");' },
  { name: "dynamic-import", source: 'const x = await import("./mod.js");' },
  { name: "optional-chain", source: 'window?.location?.href;' },
  { name: "inline-script-boundary", source: 'const s = "</script>";' },
];
