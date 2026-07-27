import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  DEPTH_PRECISION_CONSOLE_PREFIX,
  createDepthPrecisionDiagnosticArtifact,
  parsePresentedDepthPrecisionConsoleLine,
  validateDepthPrecisionDiagnostic,
} from "../src/depth-precision-diagnostic.mjs";

const here = dirname(fileURLToPath(import.meta.url));

function frame(cameraRevision = 7) {
  return {
    frame_presented: true,
    camera_revision: cameraRevision,
    applied_order_revision: cameraRevision,
    presented_order_revision_lag: 0,
    order_backend: "gpu",
    projected_execution: "compact",
    gpu_order_producer: "preproject",
    raster_execution_plan: "projected_quads_exact",
  };
}

function receipt(presentationSequence, cameraRevision = 7, orderGeneration = 9) {
  return {
    record_type: "presented_depth_precision",
    depth_precision_profile: "CandidateStable20",
    scene_generation: 1,
    camera_revision: cameraRevision,
    viewport_generation: 2,
    contract_generation: 3,
    plan_set_generation: 4,
    plan_id: "GpuPreproject",
    order_generation: orderGeneration,
    presentation_sequence: presentationSequence,
  };
}

function validEvidence() {
  return {
    frames: [frame(), frame(), frame()],
    receipts: [receipt(1), receipt(2), receipt(3)],
    runtime: { raster_path: "packed_atlas" },
  };
}

test("console parser extracts only the diagnostic record", () => {
  assert.equal(parsePresentedDepthPrecisionConsoleLine("unrelated"), null);
  const expected = receipt(1);
  assert.deepEqual(
    parsePresentedDepthPrecisionConsoleLine(`log: ${DEPTH_PRECISION_CONSOLE_PREFIX}${JSON.stringify(expected)}`),
    expected,
  );
  assert.throws(
    () => parsePresentedDepthPrecisionConsoleLine(`${DEPTH_PRECISION_CONSOLE_PREFIX}{`),
    /not JSON/,
  );
});

test("valid diagnostic is explicitly ineligible for full-quality evidence", () => {
  const evidence = validEvidence();
  const artifact = createDepthPrecisionDiagnosticArtifact({
    identity: { commit: "a".repeat(40), dirty: false },
    inputs: { dataset: "minimal_binary" },
    ...evidence,
  });
  assert.equal(artifact.evidence_class, "diagnostic");
  assert.equal(artifact.full_quality_eligible, false);
  assert.equal(artifact.validator, "diagnostic_only");
  assert.equal(artifact.frames.length, 3);
});

test("missing wrong duplicate regressing and mismatched receipts fail closed", () => {
  const cases = [
    { mutate: (value) => value.receipts.pop(), message: /exactly one/ },
    { mutate: (value) => { value.receipts[0].depth_precision_profile = "ExactFull32"; }, message: /CandidateStable20/ },
    { mutate: (value) => { value.receipts[0].plan_id = "GpuPostSort"; }, message: /GpuPreproject/ },
    { mutate: (value) => { value.receipts[1].presentation_sequence = 1; }, message: /strictly increasing/ },
    { mutate: (value) => { value.receipts[2].order_generation = 8; }, message: /move backwards/ },
    { mutate: (value) => { value.receipts[0].camera_revision = 8; }, message: /does not match/ },
    { mutate: (value) => { value.frames[0].order_backend = "cpu"; }, message: /gpu/ },
    { mutate: (value) => { value.frames[0].applied_order_revision = 6; }, message: /current camera order/ },
    { mutate: (value) => { value.receipts[0].scene_generation = Number.MAX_SAFE_INTEGER + 1; }, message: /safe integer/ },
  ];
  for (const { mutate, message } of cases) {
    const value = structuredClone(validEvidence());
    mutate(value);
    assert.throws(() => validateDepthPrecisionDiagnostic(value), message);
  }
});

test("collector source cannot route diagnostics into formal evidence", async () => {
  const source = await readFile(
    resolve(here, "../scripts/collect-web-depth-precision-diagnostic.mjs"),
    "utf8",
  );
  const forbidden = [
    "validate-" + "full-quality-experiment",
    "collect-web-" + "benchmark-artifact",
    "suite" + ".json",
  ];
  for (const token of forbidden) assert.equal(source.includes(token), false, token);
});
