import { createHash } from "node:crypto";
import { resolve } from "node:path";

export const Q1_BROWSER_PROCESS_ARGS_SCHEMA = "gsplat-q1-browser-process-args/v1";
export const Q1_BROWSER_RUNTIME_SCHEMA = "gsplat-q1-browser-runtime/v1";
export const Q1_SURFACE_DEVICE_SCHEMA = "gsplat-renderer-surface-device/v1";

function sha256Json(value) {
  return createHash("sha256").update(JSON.stringify(value)).digest("hex");
}

function text(value, label) {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new TypeError(`${label} must be a non-empty string`);
  }
  return value.trim();
}

export function browserProcessArgsReceipt({
  spawnfile,
  spawnargs,
  expectedExecutable,
  requiredArgs,
}) {
  if (resolve(text(spawnfile, "browser spawnfile"))
      !== resolve(text(expectedExecutable, "expected browser executable"))
      || !Array.isArray(spawnargs) || spawnargs.length === 0
      || spawnargs.some((argument) => typeof argument !== "string")
      || spawnargs.some((argument) => argument.startsWith("--headless"))
      || requiredArgs.some((argument) => !spawnargs.includes(argument))) {
    throw new TypeError("Q1 browser process is not the required headful launch");
  }
  const redactions = [];
  const normalizedArgs = spawnargs.map((argument, index) => {
    if (index === 0 && resolve(argument) === resolve(expectedExecutable)) {
      return "<browser-executable>";
    }
    if (argument.startsWith("--user-data-dir=")) {
      if (argument.length === "--user-data-dir=".length) {
        throw new TypeError("Q1 browser profile directory is empty");
      }
      redactions.push({ index, kind: "ephemeral_user_data_dir" });
      return "--user-data-dir=<ephemeral-profile>";
    }
    if (/^--remote-debugging-port=\d+$/.test(argument)) {
      redactions.push({ index, kind: "ephemeral_remote_debugging_port" });
      return "--remote-debugging-port=<ephemeral-port>";
    }
    return argument;
  });
  return {
    schema: Q1_BROWSER_PROCESS_ARGS_SCHEMA,
    source: "node_child_process_spawnargs",
    normalized_args: normalizedArgs,
    redactions,
    normalized_sha256: sha256Json(normalizedArgs),
  };
}

export function validateBrowserRuntimeReceipt(receipt, phase) {
  const dimensions = [
    receipt?.inner_width,
    receipt?.inner_height,
    receipt?.visual_viewport_width,
    receipt?.visual_viewport_height,
    receipt?.canvas_css_width,
    receipt?.canvas_css_height,
    receipt?.canvas_backing_width,
    receipt?.canvas_backing_height,
  ];
  if (receipt?.schema !== Q1_BROWSER_RUNTIME_SCHEMA
      || receipt.phase !== phase
      || dimensions.some((value, index) => value !== (index % 2 === 0 ? 1920 : 1080))
      || receipt.device_pixel_ratio !== 1
      || receipt.visibility_state !== "visible"
      || receipt.document_has_focus !== true) {
    throw new TypeError(`Q1 ${phase} browser runtime is not visible focused 1920x1080 DPR-1`);
  }
  return receipt;
}

export function assertStableBrowserRuntime(pre, post) {
  validateBrowserRuntimeReceipt(pre, "pre_measurement");
  validateBrowserRuntimeReceipt(post, "post_measurement");
  const stableFields = [
    "inner_width", "inner_height", "visual_viewport_width", "visual_viewport_height",
    "canvas_css_width", "canvas_css_height", "canvas_backing_width",
    "canvas_backing_height", "device_pixel_ratio",
  ];
  for (const field of stableFields) {
    if (pre[field] !== post[field]) throw new TypeError(`Q1 browser runtime ${field} drifted`);
  }
  return { pre, post };
}

export function assertStableRendererSurfaceDevice(pre, post) {
  for (const [phase, receipt] of [["pre", pre], ["post", post]]) {
    const adapter = receipt?.adapter;
    const limits = receipt?.effectiveDeviceLimits;
    if (receipt?.schema !== Q1_SURFACE_DEVICE_SCHEMA
        || receipt.provenance !== "renderer_owned_surface_session"
        || receipt.adapterSelectionClass !== "high_performance"
        || receipt.geometryPath !== "packed_atlas"
        || !Number.isSafeInteger(receipt.addressableSplatCount)
        || receipt.addressableSplatCount <= 0
        || typeof adapter?.name !== "string"
        || adapter.backend !== "browser_webgpu"
        || adapter.identityStatus !== "unavailable_wgpu28_web_backend"
        || adapter.name !== ""
        || !Number.isSafeInteger(adapter.vendorId) || adapter.vendorId < 0
        || !Number.isSafeInteger(adapter.deviceId) || adapter.deviceId < 0
        || !receipt.supportedAdapterLimits
        || typeof receipt.supportedAdapterLimits !== "object"
        || Array.isArray(receipt.supportedAdapterLimits)
        || Object.keys(receipt.supportedAdapterLimits).length === 0
        || !limits || typeof limits !== "object" || Array.isArray(limits)
        || Object.keys(limits).length === 0
        || [...Object.entries(receipt.supportedAdapterLimits), ...Object.entries(limits)]
          .some(([name, value]) =>
          !/^[a-z][A-Za-z0-9]*$/.test(name)
            || !Number.isSafeInteger(value) || value < 0)) {
      throw new TypeError(`Q1 ${phase} renderer Surface device receipt is invalid`);
    }
  }
  if (JSON.stringify(pre) !== JSON.stringify(post)) {
    throw new TypeError("Q1 renderer Surface adapter/device receipt drifted");
  }
  return post;
}

export function observedRunContext({ declared, environment, buildArtifacts }) {
  if (!declared || typeof declared !== "object" || Array.isArray(declared)
      || !declared.pairing || typeof declared.pairing !== "object"
      || !/^[0-9a-f]{64}$/.test(declared.configuration_sha256 ?? "")
      || Object.keys(declared).sort().join(",") !== "configuration_sha256,pairing") {
    throw new TypeError("Q1 run context must predeclare pairing and configuration only");
  }
  if (!environment || !buildArtifacts) {
    throw new TypeError("Q1 observed environment and build artifacts are required");
  }
  return {
    pairing: { ...declared.pairing },
    configuration_sha256: declared.configuration_sha256,
    environment,
    build_artifacts: buildArtifacts,
  };
}
