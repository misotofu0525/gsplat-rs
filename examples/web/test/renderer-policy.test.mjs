import assert from "node:assert/strict";
import test from "node:test";

import {
  rendererFailurePolicy,
  sampledWebglOptIn,
} from "../src/renderer-policy.mjs";

test("sampled WebGL fallback is disabled unless explicitly opted in", () => {
  assert.equal(sampledWebglOptIn(new URLSearchParams()), false);
  assert.equal(
    rendererFailurePolicy({
      sampledWebglEnabled: false,
      formalEvidenceRequested: false,
    }),
    "fail_closed",
  );
});

test("sampled WebGL fallback accepts an explicit diagnostic opt-in", () => {
  assert.equal(
    sampledWebglOptIn(new URLSearchParams("gsplat_allow_sampled_webgl=true")),
    true,
  );
  assert.equal(
    rendererFailurePolicy({
      sampledWebglEnabled: true,
      formalEvidenceRequested: false,
    }),
    "sampled_webgl_diagnostic",
  );
});

test("formal evidence remains fail closed even with diagnostic opt-in", () => {
  assert.equal(
    rendererFailurePolicy({
      sampledWebglEnabled: true,
      formalEvidenceRequested: true,
    }),
    "fail_closed",
  );
});

test("sampled WebGL opt-in rejects ambiguous values", () => {
  assert.throws(
    () => sampledWebglOptIn(new URLSearchParams("gsplat_allow_sampled_webgl=auto")),
    /must be true or false/,
  );
});
