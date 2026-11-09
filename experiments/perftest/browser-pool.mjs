/**
 * Configurable bounded request-level Web Worker pool.
 *
 * A browser page / dedicated worker may use this pool. A Service Worker
 * CANNOT spawn Web Workers; service-worker integration needs an opt-in
 * message broker hosted by a live controlled client, with local fallback.
 *
 * Protocol: request {id, payload} -> response {id, ok, result?, error?}
 * Every worker processes independent requests; no cross-user scanner state.
 */
export class RequestPool {
  #workers = [];
  #workerFactory;
  #pending = new Map();
  #queue = [];
  #nextId = 1;
  #closed = false;
  #maxQueue;
  #timeoutMs;

  constructor({ workerFactory, workers = 2, maxQueue = 1024, timeoutMs = 60000 }) {
    if (typeof workerFactory !== "function") throw new TypeError("workerFactory required");
    if (!Number.isSafeInteger(workers) || workers < 1) throw new RangeError("workers must be >= 1");
    if (!Number.isSafeInteger(maxQueue) || maxQueue < 0) throw new RangeError("maxQueue must be >= 0");
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 0) throw new RangeError("timeoutMs must be a non-negative integer");
    this.#timeoutMs = timeoutMs;
    this.#workerFactory = workerFactory;
    this.#maxQueue = maxQueue;
    try {
      for (let index = 0; index < workers; index++) {
        const slot = { worker: null, busy: false, id: null, index, timer: null };
        this.#start(slot);
        this.#workers.push(slot);
      }
    } catch (error) {
      this.close();
      throw error;
    }
  }

  #start(slot) {
    const worker = this.#workerFactory(slot.index);
    slot.worker = worker;
    slot.busy = false;
    slot.id = null;
    // Ignore late messages from terminated/replaced workers.
    worker.addEventListener("message", (event) => {
      if (slot.worker === worker && !this.#closed) this.#complete(slot, event.data);
    });
    for (const name of ["error", "messageerror"]) {
      worker.addEventListener(name, (event) => {
        if (slot.worker === worker && !this.#closed) {
          this.#fail(slot, new Error(event.message || name));
        }
      });
    }
  }

  submit(payload, transfer = []) {
    if (this.#closed) return Promise.reject(new Error("pool closed"));
    if (this.#queue.length >= this.#maxQueue &&
        this.#workers.every(slot => slot.busy)) {
      return Promise.reject(new Error("request queue saturated"));
    }
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      const task = { id, payload, transfer, resolve, reject };
      this.#pending.set(id, task);
      this.#queue.push(task);
      this.#drain();
    });
  }

  #drain() {
    if (this.#closed) return;
    for (const slot of this.#workers) {
      if (slot.busy || this.#queue.length === 0) continue;
      const task = this.#queue.shift();
      slot.busy = true;
      slot.id = task.id;
      try {
        // A stalled worker is replaced so subsequent independent requests
        // can make progress. The timeout starts only after dispatch.
        if (this.#timeoutMs) {
          slot.timer = setTimeout(() => {
            if (!this.#closed && slot.id === task.id) {
              this.#fail(slot, new Error("worker request timed out"));
            }
          }, this.#timeoutMs);
        }
        slot.worker.postMessage({ id: task.id, payload: task.payload }, task.transfer);
      } catch (error) {
        this.#fail(slot, error);
      }
    }
  }

  #fail(slot, error) {
    clearTimeout(slot.timer);
    slot.timer = null;
    if (slot.id !== null) {
      const task = this.#pending.get(slot.id);
      this.#pending.delete(slot.id);
      task?.reject(error);
    }
    slot.busy = false;
    slot.id = null;
    try {
      slot.worker?.terminate();
      this.#start(slot);
    } catch (startError) {
      this.close();
      return;
    }
    this.#drain();
  }

  #complete(slot, message) {
    if (!slot.busy || message?.id !== slot.id) {
      this.#fail(slot, new Error("worker response ID mismatch"));
      return;
    }
    clearTimeout(slot.timer);
    slot.timer = null;
    const task = this.#pending.get(slot.id);
    this.#pending.delete(slot.id);
    slot.busy = false;
    slot.id = null;
    if (message.ok) task?.resolve(message.result);
    else task?.reject(new Error(message.error || "worker request failed"));
    this.#drain();
  }

  close() {
    if (this.#closed) return;
    this.#closed = true;
    for (const task of this.#pending.values()) task.reject(new Error("pool closed"));
    this.#pending.clear();
    this.#queue.length = 0;
    for (const slot of this.#workers) {
      clearTimeout(slot.timer);
      slot.timer = null;
      slot.worker?.terminate();
    }
    this.#workers.length = 0;
  }
}
