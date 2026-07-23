import assert from "node:assert/strict";
import test from "node:test";

import { LatestAsyncRequestCoordinator } from "../src/latest-async-request.mjs";

test("latest async request is serialized and only the newest completion publishes", async () => {
  let releaseFirst;
  const firstGate = new Promise((resolve) => {
    releaseFirst = resolve;
  });
  let active = 0;
  let maxActive = 0;
  const performed = [];
  const published = [];
  const pending = [];
  const failures = [];
  const coordinator = new LatestAsyncRequestCoordinator({
    async perform(value) {
      active += 1;
      maxActive = Math.max(maxActive, active);
      performed.push(value);
      if (value === "first") await firstGate;
      active -= 1;
      return `${value}:done`;
    },
    publish(value, result) {
      published.push([value, result]);
    },
    onError(error) {
      failures.push(error);
    },
    onPendingChange(value) {
      pending.push(value);
    },
  });

  coordinator.request("first");
  coordinator.request("second");
  coordinator.request("latest");
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(performed, ["first"]);
  assert.equal(coordinator.pending, true);

  releaseFirst();
  await coordinator.whenIdle();
  assert.equal(maxActive, 1);
  assert.deepEqual(performed, ["first", "latest"]);
  assert.deepEqual(published, [["latest", "latest:done"]]);
  assert.deepEqual(failures, []);
  assert.deepEqual(pending, [true, false]);
});

test("invalidation discards stale success and failure before processing a new owner", async () => {
  let rejectStale;
  const staleGate = new Promise((_, reject) => {
    rejectStale = reject;
  });
  const published = [];
  const failures = [];
  const coordinator = new LatestAsyncRequestCoordinator({
    async perform(value) {
      if (value === "stale") await staleGate;
      return value;
    },
    publish(value) {
      published.push(value);
    },
    onError(error) {
      failures.push(error.message);
    },
  });

  coordinator.request("stale");
  await new Promise((resolve) => setImmediate(resolve));
  coordinator.invalidate();
  coordinator.request("current");
  rejectStale(new Error("obsolete renderer failed"));
  await coordinator.whenIdle();

  assert.deepEqual(published, ["current"]);
  assert.deepEqual(failures, []);
  assert.equal(coordinator.pending, false);
});

test("current request failure is fail-closed and clears queued work", async () => {
  const failures = [];
  const published = [];
  const coordinator = new LatestAsyncRequestCoordinator({
    async perform() {
      throw new Error("resize rejected");
    },
    publish(value) {
      published.push(value);
    },
    onError(error, value) {
      failures.push([error.message, value]);
    },
  });

  coordinator.request({ width: 800, height: 600 });
  await coordinator.whenIdle();

  assert.deepEqual(published, []);
  assert.deepEqual(failures, [["resize rejected", { width: 800, height: 600 }]]);
  assert.equal(coordinator.pending, false);
});
