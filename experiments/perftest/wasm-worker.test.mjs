import test from "node:test";
import assert from "node:assert/strict";

const events = new Map();
const messages = [];
globalThis.self = {
  addEventListener(kind, listener) { events.set(kind, listener); },
  postMessage(data, transfers) { messages.push({ data, transfers }); },
};
globalThis.fetch = async () => ({
  ok: true,
  arrayBuffer: async () => Uint8Array.of(0,97,115,109,1,0,0,0).buffer,
});
await import("./wasm-worker.mjs");
const dispatch = async (data) => {
  const handler = events.get("message");
  assert.equal(typeof handler, "function");
  await handler({ data });
};

test("initializes separate wasm instance and fully rewrites a request", async () => {
  await dispatch({
    type: "init",
    glueUrl: new URL("./fixtures/mock-wasm-glue.mjs", import.meta.url).href,
    wasmUrl: "https://example.test/scramjet.wasm",
  });
  await dispatch({
    id: 100,
    payload: {
      mode: "full",
      source: new TextEncoder().encode("window.location;"),
      options: {
        jsconfig: { wrapfn: "$wrap", prefix: "/proxy/" },
        flags: { sourcemaps: false },
        base: "https://example.com/",
        url: "https://example.com/script.js",
        isModule: false,
      },
    },
  });
  const { data, transfers } = messages.at(-1);
  assert.equal(data.id, 100);
  assert.equal(data.ok, true, data.error);
  assert.equal(new TextDecoder().decode(data.result), "window.location;");
  assert.equal(transfers[0], data.result.buffer);
});

test("separate fast scan mode remains supported", async () => {
  await dispatch({
    id: 101,
    payload: {
      mode: "fast",
      source: new TextEncoder().encode("location;"),
      options: { wrapfn: "$wrap", propertybase: "$prop$", propertyfn: "$wrapProp", disableComputedWrap: false },
    },
  });
  const { data } = messages.at(-1);
  assert.equal(data.id, 101);
  assert.equal(data.ok, true, data.error);
  assert.equal(data.result, "wrap(location);");
});

test("unknown modes fail without poisoning subsequent requests", async () => {
  await dispatch({ id: 102, payload: { mode: "unknown", source: new Uint8Array(), options: {} } });
  assert.equal(messages.at(-1).data.ok, false);
  await dispatch({ id: 103, payload: { mode: "full", source: new Uint8Array(), options: {jsconfig:{},flags:{},base:"",url:"",isModule:false} } });
  assert.equal(messages.at(-1).data.ok, true);
});
