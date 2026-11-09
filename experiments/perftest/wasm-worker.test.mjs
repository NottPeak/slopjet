import test from "node:test";
import assert from "node:assert/strict";

const events = new Map();
const messages = [];
globalThis.self = {
  addEventListener(kind, listener) { events.set(kind, listener); },
  postMessage(data) { messages.push(data); },
};
globalThis.fetch = async () => ({
  ok: true,
  arrayBuffer: async () => Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0).buffer,
});
await import("./wasm-worker.mjs");
async function dispatch(data) {
  const handler = events.get("message");
  assert.equal(typeof handler, "function");
  await handler({ data });
}

test("worker initializes the standalone scanner and rewrites independent requests", async () => {
  await dispatch({
    type: "init",
    glueUrl: new URL("./fixtures/mock-wasm-glue.mjs", import.meta.url).href,
    wasmUrl: "https://example.test/slopjet.wasm",
  });
  for (const id of [101, 102, 103]) {
    await dispatch({
      id,
      payload: {
        source: new TextEncoder().encode("location;"),
        options: {
          wrapfn: "$wrap", propertybase: "$property$",
          propertyfn: "$wrapProp", disableComputedWrap: false,
        },
      },
    });
    assert.deepEqual(messages.at(-1), { id, ok: true, result: "wrap(location);" });
  }
});

test("unsupported syntax fails closed without output", async () => {
  await dispatch({
    id: 104,
    payload: {
      source: new TextEncoder().encode("const x = 1;"),
      options: {
        wrapfn: "$wrap", propertybase: "$property$",
        propertyfn: "$wrapProp", disableComputedWrap: false,
      },
    },
  });
  assert.equal(messages.at(-1).id, 104);
  assert.equal(messages.at(-1).ok, false);
  assert.match(messages.at(-1).error, /Unsupported/);
});
