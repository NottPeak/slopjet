/**
 * Independent-request worker pool. No file/chunk sharing between workers.
 * Works with dedicated Web Workers; inject factory for bundler-specific URLs.
 * This is an experimental scheduler, not connected to the live proxy yet.
 *
 * Protocol: request {id, payload} -> response {id, ok, result?, error?}
 */
export class RequestPool {
  #workers = [];
  #pending = new Map();
  #queue = [];
  #nextId = 1;
  #closed = false;
  #maxQueue;
  constructor({ workerFactory, workers = 2, maxQueue = 1024 }) {
    if (typeof workerFactory !== "function") throw new TypeError("workerFactory required");
    if (!Number.isSafeInteger(workers) || workers < 1) throw new RangeError("workers must be >= 1");
    if (!Number.isSafeInteger(maxQueue) || maxQueue < 0) throw new RangeError("maxQueue must be >= 0");
    this.#maxQueue = maxQueue;
    for (let index = 0; index < workers; index++) {
      const worker = workerFactory(index);
      const slot = { worker, busy: false, id: null };
      worker.addEventListener("message", (event) => this.#complete(slot, event.data));
      worker.addEventListener("error", (event) => {
        const err = new Error(event.message || "worker error");
        this.#fail(slot, err);
      });
      this.#workers.push(slot);
    }
  }
  submit(payload, transfer = []) {
    if (this.#closed) return Promise.reject(new Error("pool closed"));
    // Queue capacity applies only to waiting tasks, not executing workers.
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
        slot.worker.postMessage({ id: task.id, payload: task.payload }, task.transfer);
      } catch (error) {
        this.#fail(slot, error);
      }
    }
  }
  #fail(slot, error) {
    if (slot.id !== null) {
      const task = this.#pending.get(slot.id);
      this.#pending.delete(slot.id);
      task?.reject(error);
    }
    slot.busy = false;
    slot.id = null;
    this.#drain();
  }
  #complete(slot, message) {
    if (!slot.busy || message?.id !== slot.id) {
      this.#fail(slot, new Error("worker response ID mismatch"));
      return;
    }
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
    for (const slot of this.#workers) slot.worker.terminate();
    this.#workers.length = 0;
  }
}
