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
const TRUCK_SHA256 = "65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c";
const TRUCK_PATH = "/tests/datasets/external/inria_3dgs/truck/point_cloud.ply";

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

function truckManifest() {
  return {
    id: "truck.ply",
    logical_id: "truck",
    source_path: TRUCK_PATH,
    sha256: TRUCK_SHA256,
    bytes: 630225580,
    splat_count: 2541226,
    sh_degree: 3,
  };
}

function truckReceipt() {
  return {
    dataset: "truck.ply",
    source_path: TRUCK_PATH,
    input_sha256: TRUCK_SHA256,
    source_bytes: 630225580,
    source_count: 2541226,
    decoded_count: 2541226,
    encoded_count: 2541226,
    resident_count: 2541226,
    addressable_count: 2541226,
    source_sh_degree: 3,
    resident_sh_degree: 3,
    sh_degree: 3,
    full_quality: true,
    source_membership: "all",
    sampling_enabled: false,
    lod_enabled: false,
    partial_scene_published: false,
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

test("formal Truck admission freezes source bytes, count, SH3, and full residency", () => {
  const genericTruck = canonicalDatasetIdentityForRequest("truck");
  assert.equal(genericTruck.sha256, null);
  assert.equal(genericTruck.splat_count, undefined);
  assert.deepEqual(validateFormalDatasetEvidenceIdentity({
    requestedLogicalId: "truck",
    expectedLogicalId: "truck",
    manifestDataset: truckManifest(),
    loadReceipt: truckReceipt(),
  }), truckManifest());

  const manifestMutations = [
    ["sha256", "0".repeat(64)],
    ["bytes", 630225579],
    ["splat_count", 2541225],
    ["sh_degree", 2],
  ];
  for (const [field, value] of manifestMutations) {
    assert.throws(
      () => validateFormalDatasetEvidenceIdentity({
        requestedLogicalId: "truck",
        expectedLogicalId: "truck",
        manifestDataset: { ...truckManifest(), [field]: value },
        loadReceipt: truckReceipt(),
      }),
      /dataset identity mismatch/,
      field,
    );
  }

  for (const [field, value] of [
    ["source_bytes", 630225579],
    ["resident_count", 2541225],
    ["resident_sh_degree", 2],
    ["full_quality", false],
    ["source_membership", "subset"],
    ["sampling_enabled", true],
    ["lod_enabled", true],
    ["partial_scene_published", true],
  ]) {
    assert.throws(
      () => validateFormalDatasetEvidenceIdentity({
        requestedLogicalId: "truck",
        expectedLogicalId: "truck",
        manifestDataset: truckManifest(),
        loadReceipt: { ...truckReceipt(), [field]: value },
      }),
      /dataset identity mismatch/,
      field,
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
