const FIXED_IDENTITIES = Object.freeze({
  showcase: Object.freeze({
    id: "kitune1.ply",
    logical_id: "kitsune",
    source_path: "/tests/datasets/external/wakufactory_kitune/kitune1.ply",
    sha256: "3bea1ec48ea91861fc8fad1df688a2cdb1db9b103735498b35d16d146f2551a2",
  }),
  minimal: Object.freeze({
    id: "minimal_ascii.ply",
    logical_id: "minimal_ascii",
    source_path: "/tests/datasets/minimal_ascii.ply",
    sha256: "6e4e0981ea7021c220d230c77053ba7428d96aeb4b6f2350d5006b298b0778ba",
  }),
  flowers: Object.freeze({
    id: "flowers_1.ply",
    logical_id: "flowers",
    source_path: "/tests/datasets/external/nvidia_flowers_1/flowers_1/flowers_1.ply",
    sha256: "3641192d70d598894f4d6ad92e0890b4a7336b43aa6af47bd35842c3e2ee8765",
  }),
  bonsai: Object.freeze({
    id: "bonsai.ply",
    logical_id: "bonsai",
    source_path: "/tests/datasets/external/inria_3dgs/bonsai/point_cloud.ply",
    sha256: null,
  }),
  truck: Object.freeze({
    id: "truck.ply",
    logical_id: "truck",
    source_path: "/tests/datasets/external/inria_3dgs/truck/point_cloud.ply",
    sha256: null,
  }),
  garden: Object.freeze({
    id: "garden.ply",
    logical_id: "garden",
    source_path: "/tests/datasets/external/inria_3dgs/garden/point_cloud.ply",
    sha256: null,
  }),
  bicycle: Object.freeze({
    id: "bicycle.ply",
    logical_id: "bicycle",
    source_path: "/tests/datasets/external/inria_3dgs/bicycle/point_cloud.ply",
    sha256: null,
  }),
  diagnostic: Object.freeze({
    id: "raster_diagnostic_v1.ply",
    logical_id: "raster_diagnostic_v1",
    source_path: "generated:raster_diagnostic_v1",
    sha256: null,
  }),
});

export const WEB_DATASET_PATHS = Object.freeze({
  ...Object.fromEntries(
    Object.entries(FIXED_IDENTITIES).map(([selector, identity]) => [selector, identity.source_path]),
  ),
  "truck-50000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n50000.ply",
  "truck-100000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n100000.ply",
  "truck-200000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n200000.ply",
  "truck-300000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n300000.ply",
  "truck-500000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n500000.ply",
  "truck-1000000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n1000000.ply",
  "truck-1500000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n1500000.ply",
  "truck-2000000": "/tests/datasets/external/ladder/inria-truck/point_cloud-n2000000.ply",
});

function fail(message) {
  throw new TypeError(`dataset identity mismatch: ${message}`);
}

function ladderIdentity(selector) {
  const count = /^truck-(\d+)$/.exec(selector)?.[1];
  if (count == null || WEB_DATASET_PATHS[selector] == null) return null;
  return {
    id: `point_cloud-n${count}.ply`,
    logical_id: selector,
    source_path: WEB_DATASET_PATHS[selector],
    sha256: null,
  };
}

export function canonicalDatasetIdentityForRequest(requestedDataset) {
  if (typeof requestedDataset !== "string" || requestedDataset.trim() === "") {
    fail("request must be a non-empty string");
  }
  const selector = requestedDataset.trim().toLowerCase();
  const canonicalSelector = ["showcase", "kitsune", "kitune", "fox"].includes(selector)
    ? "showcase"
    : selector === "flower" ? "flowers"
      : ["smoke"].includes(selector) ? "minimal"
        : selector === "raster_diagnostic_v1" ? "diagnostic"
          : selector;
  const identity = FIXED_IDENTITIES[canonicalSelector] ?? ladderIdentity(canonicalSelector);
  if (identity == null) fail(`unsupported request ${requestedDataset}`);
  return { ...identity };
}

function requireString(value, field) {
  if (typeof value !== "string" || value.length === 0) fail(`${field} is missing`);
  return value;
}

function requireSha256(value, field) {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    fail(`${field} is not lowercase SHA-256`);
  }
  return value;
}

function sameIdentity(left, right) {
  return left.id === right.id
    && left.logical_id === right.logical_id
    && left.source_path === right.source_path;
}

export function canonicalDatasetIdentityFromObservation({ id, source_path, sha256 }) {
  const observed = {
    id: requireString(id, "observed id"),
    source_path: requireString(source_path, "observed source_path"),
    sha256: requireSha256(sha256, "observed sha256"),
  };
  const candidates = [
    ...Object.values(FIXED_IDENTITIES),
    ...Object.keys(WEB_DATASET_PATHS)
      .filter((selector) => selector.startsWith("truck-"))
      .map(ladderIdentity),
  ].filter((identity) => identity.id === observed.id || identity.source_path === observed.source_path);
  if (candidates.length === 0) {
    return { ...observed, logical_id: observed.id };
  }
  const expected = candidates[0];
  if (candidates.some((candidate) => !sameIdentity(candidate, expected))) {
    fail(`observed id ${observed.id} conflicts with source_path ${observed.source_path}`);
  }
  if (observed.id !== expected.id) {
    fail(`expected filename id ${expected.id}, observed ${observed.id}`);
  }
  if (observed.source_path !== expected.source_path) {
    fail(`expected source_path ${expected.source_path}, observed ${observed.source_path}`);
  }
  if (expected.sha256 !== null && observed.sha256 !== expected.sha256) {
    fail(`expected sha256 ${expected.sha256}, observed ${observed.sha256}`);
  }
  return { ...expected, sha256: observed.sha256 };
}

function validateDatasetEvidenceAgainstExpected({
  expected,
  manifestDataset,
  loadReceipt = null,
}) {
  const fields = ["id", "logical_id", "source_path"];
  for (const field of fields) {
    const observed = manifestDataset?.[field];
    if (observed !== expected[field]) {
      fail(`requested ${expected[field]}, manifest ${field} observed ${observed ?? "missing"}`);
    }
  }
  const manifestSha256 = requireSha256(manifestDataset?.sha256, "manifest sha256");
  if (expected.sha256 !== null && manifestSha256 !== expected.sha256) {
    fail(`requested sha256 ${expected.sha256}, manifest observed ${manifestSha256}`);
  }
  if (loadReceipt !== null) {
    const receiptFields = {
      dataset: expected.id,
      source_path: expected.source_path,
      input_sha256: manifestSha256,
    };
    for (const [field, value] of Object.entries(receiptFields)) {
      if (loadReceipt?.[field] !== value) {
        fail(`manifest/request ${field} ${value}, load receipt observed ${loadReceipt?.[field] ?? "missing"}`);
      }
    }
  }
  return { ...expected, sha256: manifestSha256 };
}

export function validateDatasetEvidenceIdentity({
  requestedDataset,
  manifestDataset,
  loadReceipt = null,
}) {
  return validateDatasetEvidenceAgainstExpected({
    expected: canonicalDatasetIdentityForRequest(requestedDataset),
    manifestDataset,
    loadReceipt,
  });
}

export function validateFormalDatasetEvidenceIdentity({
  requestedLogicalId,
  expectedLogicalId,
  manifestDataset,
  loadReceipt = null,
}) {
  validateFormalDatasetLogicalRequest({ requestedLogicalId, expectedLogicalId });
  const expected = Object.values(FIXED_IDENTITIES)
    .find((identity) => identity.logical_id === expectedLogicalId);
  if (expected == null) fail(`unsupported formal logical_id ${expectedLogicalId}`);
  return validateDatasetEvidenceAgainstExpected({
    expected,
    manifestDataset,
    loadReceipt,
  });
}

export function validateFormalDatasetLogicalRequest({
  requestedLogicalId,
  expectedLogicalId,
}) {
  const requested = requireString(requestedLogicalId, "formal requested logical_id");
  const expected = requireString(expectedLogicalId, "formal expected logical_id");
  if (requested !== expected) {
    fail(`formal request must be logical_id ${expected}, observed ${requested}`);
  }
  return expected;
}
