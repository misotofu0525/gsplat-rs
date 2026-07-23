/**
 * Serializes an async mutation while retaining only the newest request that
 * arrived during the in-flight operation. Intermediate completions are never
 * published, so an older request cannot overwrite a newer desired state.
 */
export class LatestAsyncRequestCoordinator {
  #epoch = 0;
  #latest = null;
  #onError;
  #onPendingChange;
  #pending = false;
  #perform;
  #publish;
  #running = null;

  constructor({ perform, publish, onError, onPendingChange = () => {} }) {
    if (typeof perform !== "function" || typeof publish !== "function"
        || typeof onError !== "function") {
      throw new TypeError("LatestAsyncRequestCoordinator requires perform, publish, and onError");
    }
    this.#perform = perform;
    this.#publish = publish;
    this.#onError = onError;
    this.#onPendingChange = onPendingChange;
  }

  get pending() {
    return this.#pending;
  }

  request(value) {
    this.#latest = { epoch: this.#epoch, value };
    this.#setPending(true);
    this.#startIfNeeded();
  }

  invalidate() {
    this.#epoch += 1;
    this.#latest = null;
    if (!this.#running) this.#setPending(false);
  }

  async whenIdle() {
    for (;;) {
      const running = this.#running;
      if (!running) return;
      await running;
    }
  }

  #startIfNeeded() {
    if (this.#running || !this.#latest) return;
    const running = this.#drain();
    this.#running = running;
    running.then(
      () => this.#finishRun(running),
      (error) => {
        // #drain handles request failures. This guard covers an unexpected
        // coordinator callback failure without leaving pending stuck forever.
        this.#onError(error, null);
        this.#finishRun(running);
      },
    );
  }

  async #drain() {
    while (this.#latest) {
      const request = this.#latest;
      this.#latest = null;
      try {
        const result = await this.#perform(request.value);
        if (request.epoch !== this.#epoch) continue;
        if (this.#latest) continue;
        await this.#publish(request.value, result);
      } catch (error) {
        if (request.epoch !== this.#epoch) continue;
        this.#latest = null;
        await this.#onError(error, request.value);
        return;
      }
    }
  }

  #finishRun(running) {
    if (this.#running !== running) return;
    this.#running = null;
    if (this.#latest) {
      this.#startIfNeeded();
    } else {
      this.#setPending(false);
    }
  }

  #setPending(pending) {
    if (this.#pending === pending) return;
    this.#pending = pending;
    this.#onPendingChange(pending);
  }
}
