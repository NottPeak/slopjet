import test from "node:test";
import assert from "node:assert/strict";
import { RequestPool } from "./browser-pool.mjs";

class MockWorker {
  constructor() {
    this.handlers = { message: [], error: [] };
    this.closed = false;
  }
  addEventListener(name, handler) { this.handlers[name].push(handler); }
  postMessage({ id, payload }) {
    queueMicrotask(() => {
      const message = { id, ok: payload !== "fail", result: payload * 2, error: "failure" };
      for (const fn of this.handlers.message) fn({ data: message });
    });
  }
  terminate() { this.closed = true; }
}
test("orders results for many independent requests", async () => {
  const pool = new RequestPool({ workerFactory: () => new MockWorker(), workers: 3 });
  const values = await Promise.all(Array.from({ length: 100 }, (_, i) => pool.submit(i)));
  assert.deepEqual(values, Array.from({ length: 100 }, (_, i) => i * 2));
  pool.close();
});

test("one failed job does not poison other requests", async () => {
  const pool = new RequestPool({ workerFactory: () => new MockWorker(), workers: 2 });
  const results = await Promise.allSettled([pool.submit(1), pool.submit("fail"), pool.submit(3)]);
  assert.equal(results[0].status, "fulfilled");
  assert.equal(results[1].status, "rejected");
  assert.equal(results[2].status, "fulfilled");
  pool.close();
});

test("closed pools reject new requests", async () => {
  const pool = new RequestPool({ workerFactory: () => new MockWorker(), workers: 1 });
  pool.close();
  await assert.rejects(pool.submit(1), /closed/);
});

test("backpressure rejects when queue is full", async () => {
  const pool = new RequestPool({ workerFactory: () => new MockWorker(), workers: 1, maxQueue: 0 });
  const first = pool.submit(1);
  await assert.rejects(pool.submit(2), /saturated/);
  assert.equal(await first, 2);
  pool.close();
});
