import assert from "node:assert/strict";
import test from "node:test";

import {
  canonicalDatasetIdentityFromObservation,
  canonicalDatasetIdentityForRequest,
  validateDatasetEvidenceIdentity,
  validateFormalDatasetEvidenceIdentity,
} from "../src/dataset-identity.mjs";

const KITSUNE_SHA256 = "3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2";
const KITSUNE_PATH = "/tests/datasets/external/wakufactory_kitune/kitune1.ply";

function kitsuneManifest() {
  return {
    id: "kitune1.ply",
    logical_id: "kitsune",
    source_path: KITSUNE_PATH,
    sha256: KITSUNE_SHA256,
  };
}

function kitsuneReceipt() {
  return {
    dataset: "kitune1.ply",
    source_path: KITSUNE_PATH,
    input_sha256: KITSUNE_SHA256,
  };
}

test("canonical Kitsune request, manifest, and observed load receipt bind", () => {
  assert.deepEqual(canonicalDatasetIdentityForRequest("kitsune"), kitsuneManifest());
  assert.deepEqual(
    canonicalDatasetIdentityFromObservation({
      id: "kitune1.ply",
      source_path: KITSUNE_PATH,
      sha256: KITSUNE_SHA256,
    }),
    kitsuneManifest(),
  );
  assert.deepEqual(validateDatasetEvidenceIdentity({
    requestedDataset: "kitsune",
    manifestDataset: kitsuneManifest(),
    loadReceipt: kitsuneReceipt(),
  }), kitsuneManifest());
});

test("formal Kitsune admission requires the exact canonical logical request", () => {
  assert.deepEqual(validateFormalDatasetEvidenceIdentity({
    requestedLogicalId: "kitsune",
    expectedLogicalId: "kitsune",
    manifestDataset: kitsuneManifest(),
    loadReceipt: kitsuneReceipt(),
  }), kitsuneManifest());

  for (const alias of ["fox", "showcase", "kitune"]) {
    assert.throws(
      () => validateFormalDatasetEvidenceIdentity({
        requestedLogicalId: alias,
        expectedLogicalId: "kitsune",
        manifestDataset: kitsuneManifest(),
        loadReceipt: kitsuneReceipt(),
      }),
      new RegExp(`formal request must be logical_id kitsune, observed ${alias}`),
    );
  }
});

test("Kitsune UI aliases retain their selection identity", () => {
  for (const alias of ["showcase", "kitsune", "kitune", "fox"]) {
    assert.deepEqual(canonicalDatasetIdentityForRequest(alias), kitsuneManifest());
  }
});

test("dataset evidence rejects a logical alias in the filename field and a conflicting path", () => {
  assert.throws(
    () => validateDatasetEvidenceIdentity({
      requestedDataset: "kitsune",
      manifestDataset: { ...kitsuneManifest(), id: "kitsune" },
      loadReceipt: kitsuneReceipt(),
    }),
    /requested kitune1\.ply, manifest id observed kitsune/,
  );
  assert.throws(
    () => validateDatasetEvidenceIdentity({
      requestedDataset: "kitsune",
      manifestDataset: { ...kitsuneManifest(), logical_id: "fox" },
      loadReceipt: kitsuneReceipt(),
    }),
    /requested kitsune, manifest logical_id observed fox/,
  );
  assert.throws(
    () => validateDatasetEvidenceIdentity({
      requestedDataset: "kitsune",
      manifestDataset: kitsuneManifest(),
      loadReceipt: { ...kitsuneReceipt(), source_path: "/tmp/kitune1.ply" },
    }),
    /load receipt observed \/tmp\/kitune1\.ply/,
  );
  assert.throws(
    () => validateDatasetEvidenceIdentity({
      requestedDataset: "kitsune",
      manifestDataset: { ...kitsuneManifest(), source_path: "/tmp/kitune1.ply" },
      loadReceipt: kitsuneReceipt(),
    }),
    /manifest source_path observed \/tmp\/kitune1\.ply/,
  );
});

test("dataset evidence rejects mismatched manifest and receipt SHA-256", () => {
  assert.throws(
    () => validateDatasetEvidenceIdentity({
      requestedDataset: "kitsune",
      manifestDataset: { ...kitsuneManifest(), sha256: "0".repeat(64) },
      loadReceipt: kitsuneReceipt(),
    }),
    /requested sha256 .* manifest observed/,
  );
  assert.throws(
    () => validateDatasetEvidenceIdentity({
      requestedDataset: "kitsune",
      manifestDataset: kitsuneManifest(),
      loadReceipt: { ...kitsuneReceipt(), input_sha256: "0".repeat(64) },
    }),
    /load receipt observed/,
  );
});

test("minimal collector identity remains valid", () => {
  const expected = canonicalDatasetIdentityForRequest("minimal");
  assert.deepEqual(validateDatasetEvidenceIdentity({
    requestedDataset: "minimal",
    manifestDataset: expected,
    loadReceipt: {
      dataset: expected.id,
      source_path: expected.source_path,
      input_sha256: expected.sha256,
    },
  }), expected);
});
