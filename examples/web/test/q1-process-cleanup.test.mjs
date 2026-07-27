import assert from "node:assert/strict";
import test from "node:test";

import { cleanupBrowserAndServer } from "../scripts/q1-process-cleanup.mjs";

test("Q1 cleanup reports a browser close exception even after the process exited", async () => {
  const browser = {
    process: () => ({ exitCode: 0, signalCode: null }),
    close: async () => { throw new Error("injected browser close failure"); },
  };
  const server = { exitCode: 0, signalCode: null };
  await assert.rejects(
    cleanupBrowserAndServer(browser, server),
    /cleanup was not stable/,
  );
});

test("Q1 cleanup accepts already-exited browser and server processes", async () => {
  const browser = {
    process: () => ({ exitCode: 0, signalCode: null }),
    close: async () => {},
  };
  const server = { exitCode: 0, signalCode: null };
  await assert.doesNotReject(cleanupBrowserAndServer(browser, server));
});
