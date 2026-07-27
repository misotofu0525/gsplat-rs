import assert from "node:assert/strict";
import test from "node:test";

import {
  assertStableBrowserRuntime,
  browserProcessArgsReceipt,
  observedRunContext,
} from "../scripts/q1-browser-environment.mjs";

const runtime = (phase) => ({
  schema: "gsplat-q1-browser-runtime/v1",
  phase,
  inner_width: 1920,
  inner_height: 1080,
  visual_viewport_width: 1920,
  visual_viewport_height: 1080,
  canvas_css_width: 1920,
  canvas_css_height: 1080,
  canvas_backing_width: 1920,
  canvas_backing_height: 1080,
  device_pixel_ratio: 1,
  visibility_state: "visible",
  document_has_focus: true,
});

test("Q1 browser receipt binds actual headful spawnargs with ephemeral values redacted", () => {
  const receipt = browserProcessArgsReceipt({
    spawnfile: "/Applications/Chrome",
    spawnargs: [
      "/Applications/Chrome",
      "--enable-gpu",
      "--user-data-dir=/tmp/profile",
      "--remote-debugging-port=1234",
    ],
    expectedExecutable: "/Applications/Chrome",
    requiredArgs: ["--enable-gpu"],
  });
  assert.equal(receipt.schema, "gsplat-q1-browser-process-args/v1");
  assert.deepEqual(receipt.normalized_args.slice(-2), [
    "--user-data-dir=<ephemeral-profile>",
    "--remote-debugging-port=<ephemeral-port>",
  ]);
  assert.throws(() => browserProcessArgsReceipt({
    spawnfile: "/Applications/Chrome",
    spawnargs: ["/Applications/Chrome", "--enable-gpu", "--headless=new"],
    expectedExecutable: "/Applications/Chrome",
    requiredArgs: ["--enable-gpu"],
  }), /headful/);
});

test("Q1 runtime requires stable visible focused CSS and backing 1080p", () => {
  assert.doesNotThrow(() => assertStableBrowserRuntime(
    runtime("pre_measurement"),
    runtime("post_measurement"),
  ));
  assert.throws(() => assertStableBrowserRuntime(
    { ...runtime("pre_measurement"), canvas_css_width: 1280 },
    runtime("post_measurement"),
  ), /1920x1080/);
  assert.throws(() => assertStableBrowserRuntime(
    runtime("pre_measurement"),
    { ...runtime("post_measurement"), document_has_focus: false },
  ), /visible focused/);
});

test("Q1 observed context rejects caller-supplied physical build and environment claims", () => {
  const declared = {
    pairing: { pair_id: "pair-1" },
    configuration_sha256: "a".repeat(64),
    environment: { adapter: "fabricated" },
    build_artifacts: { runtime_js: { sha256: "b".repeat(64) } },
  };
  const environment = { adapter: "observed" };
  const buildArtifacts = { runtime_js: { sha256: "c".repeat(64) } };
  assert.throws(
    () => observedRunContext({ declared, environment, buildArtifacts }),
    /pairing and configuration only/,
  );
  delete declared.environment;
  delete declared.build_artifacts;
  assert.deepEqual(observedRunContext({ declared, environment, buildArtifacts }), {
    pairing: declared.pairing,
    configuration_sha256: declared.configuration_sha256,
    environment,
    build_artifacts: buildArtifacts,
  });
});
