import assert from "node:assert/strict";
import test from "node:test";
import { access, mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { Q1ArtifactTransaction } from "../scripts/q1-artifact-transaction.mjs";

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "gsplat-q1-transaction-"));
  const cells = join(root, "cells");
  await mkdir(cells);
  return { root, final: join(cells, "pair-1-control") };
}

async function missing(path) {
  await assert.rejects(access(path), { code: "ENOENT" });
}

test("Q1 transaction rejects an existing exact destination before staging", async () => {
  const { root, final } = await fixture();
  await mkdir(final);
  await assert.rejects(Q1ArtifactTransaction.claim({
    seriesRoot: root,
    finalDirectory: final,
    collectionSessionId: "session-1",
  }), /not fresh/);
});

test("Q1 mid-write failure leaves no final and publishes one blocker", async () => {
  const { root, final } = await fixture();
  const transaction = await Q1ArtifactTransaction.claim({
    seriesRoot: root,
    finalDirectory: final,
    collectionSessionId: "session-1",
  });
  await writeFile(join(transaction.stagingDirectory, "manifest.json"), "partial");
  await transaction.recordFailure(new Error("injected mid-write"), "artifact_write");
  await missing(final);
  await access(transaction.blockerPath);
  await assert.rejects(transaction.publish(), /cannot publish/);
});

test("Q1 pre-publish failure cannot rename complete-looking staging", async () => {
  const { root, final } = await fixture();
  const transaction = await Q1ArtifactTransaction.claim({
    seriesRoot: root,
    finalDirectory: final,
    collectionSessionId: "session-1",
  });
  await writeFile(join(transaction.stagingDirectory, "manifest.json"), "complete-looking");
  transaction.markCleanupComplete();
  await transaction.recordFailure(new Error("injected before publish"), "validation");
  await assert.rejects(transaction.publish(), /cannot publish/);
  await missing(final);
});

test("Q1 browser or server cleanup failure emits cleanup blocker and forbids publication", async () => {
  const { root, final } = await fixture();
  const transaction = await Q1ArtifactTransaction.claim({
    seriesRoot: root,
    finalDirectory: final,
    collectionSessionId: "session-1",
  });
  await writeFile(join(transaction.stagingDirectory, "manifest.json"), "complete-looking");
  await transaction.recordCleanupFailure(new Error("injected browser/server cleanup"));
  await access(transaction.cleanupBlockerPath);
  await access(transaction.blockerPath);
  await assert.rejects(transaction.publish(), /cannot publish/);
  await missing(final);
});

test("Q1 publishes atomically only after cleanup completion", async () => {
  const { root, final } = await fixture();
  const transaction = await Q1ArtifactTransaction.claim({
    seriesRoot: root,
    finalDirectory: final,
    collectionSessionId: "session-1",
  });
  await writeFile(join(transaction.stagingDirectory, "manifest.json"), "complete");
  await assert.rejects(transaction.publish(), /before stable cleanup/);
  transaction.markCleanupComplete();
  assert.equal(await transaction.publish(), final);
  await access(join(final, "manifest.json"));
  await access(transaction.claimPath);
});
