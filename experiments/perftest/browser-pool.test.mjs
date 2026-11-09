import test from "node:test";
import assert from "node:assert/strict";
import { RequestPool } from "./browser-pool.mjs";

class MockWorker {
  constructor() {
    this.handlers = { message: [], error: [], messageerror: [] };
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


test("replaces a crashed worker and continues processing other users", async () => {
  let starts = 0;
  let previous = null;
  class CrashableWorker extends MockWorker {
    postMessage({ id, payload }) {
      if (payload === "crash") {
        queueMicrotask(() => {
          for (const fn of this.handlers.error) fn({ message: "worker crashed" });
        });
        return;
      }
      super.postMessage({ id, payload });
    }
  }
  const pool = new RequestPool({
    workers: 1,
    workerFactory: () => {
      starts++;
      previous = new CrashableWorker();
      return previous;
    },
  });
  await assert.rejects(pool.submit("crash"), /crashed/);
  assert.ok(previous && starts > 1);
  assert.equal(await pool.submit(21), 42);
  pool.close();
});

test("ignores stale messages after a worker has been replaced", async () => {
  let former;
  class ReplacementWorker extends MockWorker {
    postMessage({ id, payload }) {
      if (payload === "crash") {
        queueMicrotask(() => {
          for (const fn of this.handlers.error) fn({ message: "crash" });
        });
        return;
      }
      super.postMessage({ id, payload });
    }
  }
  const pool = new RequestPool({
    workers: 1,
    workerFactory: () => {
      const worker = new ReplacementWorker();
      former ??= worker;
      return worker;
    },
  });
  await assert.rejects(pool.submit("crash"), /crash/);
  for (const fn of former.handlers.message) {
    fn({ data: { id: 1, ok: true, result: 999 } });
  }
  assert.equal(await pool.submit(3), 6);
  pool.close();
});

test("hung workers time out and do not block later users", async () => {
  let starts = 0;
  class HangingWorker extends MockWorker {
    postMessage(request) {
      if (request.payload === "hang") return;
      super.postMessage(request);
    }
  }
  const pool = new RequestPool({
    workers: 1, timeoutMs: 15,
    workerFactory: () => {
      starts++;
      return new HangingWorker();
    },
  });
  await assert.rejects(pool.submit("hang"), /timed out/);
  assert.equal(await pool.submit(11), 22);
  assert.ok(starts >= 2, "hung worker should have been replaced");
  pool.close();
});
