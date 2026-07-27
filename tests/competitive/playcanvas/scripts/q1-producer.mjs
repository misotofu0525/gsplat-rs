import { createHash } from 'node:crypto';
import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { relative, resolve } from 'node:path';
import {
  Q1_ADAPTER_IDENTITY_STATUS,
  Q1_ADAPTER_SELECTION_CLASS,
  Q1_CANONICAL_ADAPTER_SCHEMA,
  Q1_CANONICAL_SUPPORTED_LIMIT_NAMES,
  Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
  Q1_WEBGPU_ENVIRONMENT_SCHEMA
} from '../public/webgpu-environment-receipt.js';

export const Q1_PRODUCER_REQUEST_SCHEMA = 'gsplat-q1-playcanvas-producer-request/v1';
export const Q1_TERMINAL_SCHEMA = 'gsplat-q1-webgpu-terminal-window/v1';
export const Q1_BROWSER_PROCESS_ARGS_SCHEMA = 'gsplat-q1-browser-process-args/v1';
const SHA256_PATTERN = /^[0-9a-f]{64}$/;
const CONTROL_TRACES = new Set([0, 1]);

function requiredText(value, label) {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  return value.trim();
}

function requiredSha(value, label) {
  const digest = requiredText(value, label);
  if (!SHA256_PATTERN.test(digest)) throw new Error(`${label} must be a lowercase SHA-256`);
  return digest;
}

function requiredObject(value, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value;
}

function normalizedLimits(value, label, requiredNames = []) {
  const source = requiredObject(value, label);
  const result = {};
  for (const name of Object.keys(source).sort()) {
    const limit = source[name];
    if (!Number.isSafeInteger(limit) || limit < 0) {
      throw new Error(`${label}.${name} must be a non-negative safe integer`);
    }
    result[name] = limit;
  }
  if (Object.keys(result).length === 0) throw new Error(`${label} must not be empty`);
  for (const name of requiredNames) {
    if (!Number.isSafeInteger(result[name]) || result[name] <= 0) {
      throw new Error(`${label}.${name} is required for the canonical WebGPU limit contract`);
    }
  }
  return result;
}

export function normalizeQ1WebGpuEnvironmentReceipt(value) {
  const receipt = requiredObject(value, 'WebGPU environment receipt');
  if (receipt.schema !== Q1_WEBGPU_ENVIRONMENT_SCHEMA || receipt.endpoint !== 'playcanvas') {
    throw new Error('Q1 run lacks the PlayCanvas renderer-selected WebGPU environment receipt');
  }
  const selectedAdapter = requiredObject(receipt.selected_adapter, 'selected adapter receipt');
  if (selectedAdapter.provenance !== 'playcanvas_graphicsDevice.gpuAdapter' ||
      !['browser_exposed', 'browser_redacted'].includes(selectedAdapter.info_status)) {
    throw new Error('Q1 selected adapter provenance is invalid');
  }
  const adapterInfo = requiredObject(selectedAdapter.info, 'selected adapter info');
  const expectedInfoNames = [
    'vendor', 'architecture', 'device', 'description', 'subgroupMinSize', 'subgroupMaxSize'
  ];
  if (Object.keys(adapterInfo).sort().join('\0') !== [...expectedInfoNames].sort().join('\0')) {
    throw new Error('Q1 selected adapter info fields are incomplete');
  }
  for (const name of expectedInfoNames) {
    const field = adapterInfo[name];
    if (field !== null && typeof field !== 'string' && !Number.isSafeInteger(field)) {
      throw new Error(`Q1 selected adapter info ${name} has an invalid value`);
    }
  }
  const adapterInfoExposed = Object.values(adapterInfo).some((field) =>
    (typeof field === 'string' && field.trim().length > 0) || Number.isSafeInteger(field)
  );
  if ((selectedAdapter.info_status === 'browser_exposed') !== adapterInfoExposed) {
    throw new Error('Q1 selected adapter info status does not match its fields');
  }
  const adapterSupportedLimits = normalizedLimits(
    selectedAdapter.supported_limits,
    'selected adapter supported limits',
    Q1_CANONICAL_SUPPORTED_LIMIT_NAMES
  );
  const selectedDevice = requiredObject(receipt.selected_device, 'selected device receipt');
  if (selectedDevice.provenance !== 'playcanvas_graphicsDevice.wgpu') {
    throw new Error('Q1 selected device provenance is invalid');
  }
  const deviceEffectiveLimits = normalizedLimits(
    selectedDevice.effective_limits,
    'selected device effective limits',
    Q1_CANONICAL_SUPPORTED_LIMIT_NAMES
  );
  const canonical = requiredObject(receipt.canonical_adapter, 'canonical adapter receipt');
  const expectedCanonical = {
    schema: Q1_CANONICAL_ADAPTER_SCHEMA,
    selection_class: Q1_ADAPTER_SELECTION_CLASS,
    backend_class: 'browser_webgpu',
    hardware_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
    supported_limits_schema: Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA
  };
  if (Object.keys(canonical).sort().join('\0') !==
      [...Object.keys(expectedCanonical), 'supported_limits'].sort().join('\0')) {
    throw new Error('Q1 canonical adapter fields are not exact');
  }
  for (const [name, expected] of Object.entries(expectedCanonical)) {
    if (canonical[name] !== expected) {
      throw new Error(`Q1 canonical adapter ${name} is invalid`);
    }
  }
  const canonicalSupportedLimits = normalizedLimits(
    canonical.supported_limits,
    'canonical adapter supported limits',
    Q1_CANONICAL_SUPPORTED_LIMIT_NAMES
  );
  if (Object.keys(canonicalSupportedLimits).join('\0') !==
      Q1_CANONICAL_SUPPORTED_LIMIT_NAMES.join('\0')) {
    throw new Error('Q1 canonical adapter supported-limit set is not exact');
  }
  for (const name of Q1_CANONICAL_SUPPORTED_LIMIT_NAMES) {
    if (canonicalSupportedLimits[name] !== adapterSupportedLimits[name]) {
      throw new Error(`Q1 canonical adapter supported limit ${name} is not selected-adapter data`);
    }
  }
  return {
    schema: Q1_WEBGPU_ENVIRONMENT_SCHEMA,
    endpoint: 'playcanvas',
    selected_adapter: {
      provenance: selectedAdapter.provenance,
      info_status: selectedAdapter.info_status,
      info: Object.fromEntries(expectedInfoNames.map((name) => [name, adapterInfo[name]])),
      supported_limits: adapterSupportedLimits
    },
    selected_device: {
      provenance: selectedDevice.provenance,
      effective_limits: deviceEffectiveLimits
    },
    canonical_adapter: {
      ...expectedCanonical,
      supported_limits: canonicalSupportedLimits
    }
  };
}

function inside(root, path, label) {
  const resolvedRoot = resolve(root);
  const resolvedPath = resolve(path);
  const pathFromRoot = relative(resolvedRoot, resolvedPath);
  if (pathFromRoot === '' || pathFromRoot === '..' || pathFromRoot.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`)) {
    if (pathFromRoot === '') return resolvedPath;
    throw new Error(`${label} must remain inside the Q1 series root`);
  }
  return resolvedPath;
}

function controlBindings(value, configurationSha256) {
  if (!Array.isArray(value) || value.length !== 2) {
    throw new Error('throughput request must bind exactly two controls');
  }
  const result = value.map((binding, index) => {
    if (!binding || typeof binding !== 'object' || Array.isArray(binding)) {
      throw new Error(`control_bindings[${index}] must be an object`);
    }
    const trace = binding.trace_frame_index;
    if (!CONTROL_TRACES.has(trace)) {
      throw new Error(`control_bindings[${index}].trace_frame_index must be 0 or 1`);
    }
    const configuration = requiredSha(
      binding.configuration_sha256,
      `control_bindings[${index}].configuration_sha256`
    );
    if (configuration !== configurationSha256) {
      throw new Error('throughput control binding configuration does not match the request');
    }
    return {
      trace_frame_index: trace,
      run_id: requiredText(binding.run_id, `control_bindings[${index}].run_id`),
      manifest_sha256: requiredSha(
        binding.manifest_sha256,
        `control_bindings[${index}].manifest_sha256`
      ),
      configuration_sha256: configuration
    };
  }).sort((left, right) => left.trace_frame_index - right.trace_frame_index);
  if (result[0].trace_frame_index !== 0 || result[1].trace_frame_index !== 1) {
    throw new Error('throughput control bindings must cover traces 0 and 1 exactly once');
  }
  return result;
}

export function validateQ1ProducerRequest(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      value.schema !== Q1_PRODUCER_REQUEST_SCHEMA) {
    throw new Error(`Q1 producer request schema must equal ${Q1_PRODUCER_REQUEST_SCHEMA}`);
  }
  const role = requiredText(value.artifact_role, 'artifact_role');
  if (!['control', 'throughput'].includes(role)) {
    throw new Error('artifact_role must be control or throughput');
  }
  const position = value.position;
  if (!Number.isSafeInteger(position) || ![1, 2].includes(position)) {
    throw new Error('position must be 1 or 2');
  }
  const runOrder = requiredText(value.run_order, 'run_order');
  if (!['playcanvas-first', 'gsplat-rs-first'].includes(runOrder)) {
    throw new Error('run_order must be playcanvas-first or gsplat-rs-first');
  }
  const configurationSha256 = requiredSha(value.configuration_sha256, 'configuration_sha256');
  const captureTraceFrameIndex = value.capture_trace_frame_index ?? null;
  if (role === 'control' && !CONTROL_TRACES.has(captureTraceFrameIndex)) {
    throw new Error('control request capture_trace_frame_index must be 0 or 1');
  }
  if (role === 'throughput' && captureTraceFrameIndex !== null) {
    throw new Error('throughput request must not select a capture trace');
  }
  if (role === 'control' && value.control_bindings !== undefined) {
    throw new Error('control request must not contain throughput control bindings');
  }
  return {
    role,
    seriesId: requiredText(value.series_id, 'series_id'),
    scheduleSha256: requiredSha(value.schedule_sha256, 'schedule_sha256'),
    protocolSha256: requiredSha(value.protocol_sha256, 'protocol_sha256'),
    configurationSha256,
    pairId: requiredText(value.pair_id, 'pair_id'),
    runOrder,
    position,
    collectionSessionId: requiredText(value.collection_session_id, 'collection_session_id'),
    captureTraceFrameIndex,
    controlBindings: role === 'throughput'
      ? controlBindings(value.control_bindings, configurationSha256)
      : null
  };
}

export async function loadQ1ProducerConfig(environment, outputRoot) {
  const requestPath = environment.PLAYCANVAS_Q1_PRODUCER_REQUEST?.trim();
  if (!requestPath) return null;
  const seriesRootText = requiredText(
    environment.PLAYCANVAS_Q1_SERIES_ROOT,
    'PLAYCANVAS_Q1_SERIES_ROOT'
  );
  const request = validateQ1ProducerRequest(
    JSON.parse(await readFile(resolve(requestPath), 'utf8'))
  );
  const seriesRoot = resolve(seriesRootText);
  const artifactRoot = inside(seriesRoot, outputRoot, 'PLAYCANVAS_ARTIFACT_DIR');
  if (artifactRoot === seriesRoot) {
    throw new Error('Q1 artifact directory must be a child of the series root');
  }
  return { ...request, seriesRoot, artifactRoot };
}

export function assertQ1Invocation(config, invocation) {
  if (!config) return;
  const expected = {
    qualificationName: 'truck-quality-1080p-v1',
    cameraMode: 'sequence',
    warmupFrames: 20,
    measuredFrames: 80,
    viewportWidth: 1920,
    viewportHeight: 1080,
    sessionMode: 'local-launch',
    headless: false
  };
  for (const [key, value] of Object.entries(expected)) {
    if (invocation[key] !== value) throw new Error(`Q1 ${key} must equal ${value}`);
  }
  const expectedCapture = config.role === 'control' ? config.captureTraceFrameIndex : null;
  if (invocation.captureTraceFrame !== expectedCapture) {
    throw new Error('Q1 capture trace does not match the producer request role');
  }
}

export function q1RendererCaptureEnabled(config) {
  return !config || config.role === 'control';
}

export function q1BrowserProcessArgsReceipt({
  spawnfile,
  spawnargs,
  expectedExecutable,
  requiredArgs
}) {
  if (resolve(requiredText(spawnfile, 'browser process spawnfile')) !==
      resolve(requiredText(expectedExecutable, 'expected browser executable')) ||
      !Array.isArray(spawnargs) || spawnargs.length === 0 ||
      spawnargs.some((argument) => typeof argument !== 'string') ||
      spawnargs.some((argument) => argument.startsWith('--headless')) ||
      requiredArgs.some((argument) => !spawnargs.includes(argument))) {
    throw new Error('Q1 browser process does not match the verified headful launch configuration');
  }
  const redactions = [];
  const normalizedArgs = spawnargs.map((argument, index) => {
    if (index === 0 && resolve(argument) === resolve(expectedExecutable)) {
      return '<browser-executable>';
    }
    if (argument.startsWith('--user-data-dir=')) {
      if (argument.length === '--user-data-dir='.length) {
        throw new Error('Q1 browser process has an empty profile directory');
      }
      redactions.push({ index, kind: 'ephemeral_user_data_dir' });
      return '--user-data-dir=<ephemeral-profile>';
    }
    if (/^--remote-debugging-port=\d+$/.test(argument)) {
      redactions.push({ index, kind: 'ephemeral_remote_debugging_port' });
      return '--remote-debugging-port=<ephemeral-port>';
    }
    return argument;
  });
  return {
    schema: Q1_BROWSER_PROCESS_ARGS_SCHEMA,
    source: 'node_child_process_spawnargs',
    normalized_args: normalizedArgs,
    redactions,
    normalized_sha256: sha256(Buffer.from(JSON.stringify(normalizedArgs)))
  };
}

export function q1TerminalWindow(queueTerminal, measuredFrameCount, perFrameObserverReads) {
  const { sustained, measurementDrain } = queueTerminal;
  if (!Number.isSafeInteger(measuredFrameCount) || measuredFrameCount <= 0 ||
      sustained?.measuredFrameCount !== measuredFrameCount ||
      sustained.submitVersionEnd - sustained.submitVersionStart !== measuredFrameCount ||
      measurementDrain?.submitVersionStable !== true ||
      !Number.isFinite(sustained.queueTerminalSpanMs) ||
      sustained.queueTerminalSpanMs <= 0 ||
      !Number.isFinite(measurementDrain.endedAtMs) ||
      perFrameObserverReads !== 0) {
    throw new Error('Q1 throughput does not prove one continuous submission per measured frame');
  }
  const end = measurementDrain.endedAtMs;
  const duration = sustained.queueTerminalSpanMs;
  return {
    schema: Q1_TERMINAL_SCHEMA,
    clock: 'performance_now_monotonic',
    start_boundary: 'first_measured_camera_input_accepted',
    end_boundary: 'final_measured_gpu_queue_completion',
    completion_primitive: 'gpu_queue_on_submitted_work_done',
    frame_loop_policy: 'controlled_presented_raf',
    camera_mutation_point: 'before_update_order_project_render',
    warmup_queue_drained: true,
    continuous_submissions: true,
    per_frame_observer_reads: perFrameObserverReads,
    extra_submissions_during_terminal_drain: 0,
    measured_camera_input_count: measuredFrameCount,
    measured_submission_count: measuredFrameCount,
    dropped_frame_count: 0,
    submission_counter_stable_during_drain: measurementDrain.submitVersionStable,
    submission_counter_before_first: sustained.submitVersionStart,
    submission_counter_after_last: sustained.submitVersionEnd,
    started_at_monotonic_ms: end - duration,
    completed_at_monotonic_ms: end,
    duration_ms: duration
  };
}

export function q1ManifestFields(
  config,
  queueTerminal,
  measuredFrameCount,
  perFrameObserverReads
) {
  if (!config) return null;
  const q1 = {
    artifact_role: config.role,
    protocol_sha256: config.protocolSha256,
    configuration_sha256: config.configurationSha256,
    performance_evidence: config.role === 'throughput',
    count_scope: 'full_membership_v_c_d_unavailable'
  };
  if (config.role === 'control') {
    q1.capture_trace_frame_index = config.captureTraceFrameIndex;
  } else {
    q1.control_bindings = config.controlBindings;
    q1.terminal_window = q1TerminalWindow(
      queueTerminal,
      measuredFrameCount,
      perFrameObserverReads
    );
  }
  return q1;
}

export function q1PairingFields(config) {
  if (!config) return null;
  return {
    series_id: config.seriesId,
    schedule_sha256: config.scheduleSha256,
    pair_id: config.pairId,
    run_order: config.runOrder,
    position: config.position,
    fresh_output: true,
    automatic_retry: false
  };
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

async function q1BuildArtifactContents(harnessRoot) {
  const runtimeRoot = resolve(harnessRoot, 'node_modules/playcanvas/build/playcanvas');
  const runtimeFiles = [];
  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) {
        await visit(path);
      } else if (entry.isFile()) {
        const bytes = await readFile(path);
        runtimeFiles.push({
          path: relative(runtimeRoot, path).split('\\').join('/'),
          bytes: bytes.length,
          sha256: sha256(bytes)
        });
      } else {
        throw new Error('PlayCanvas runtime tree contains a non-file entry');
      }
    }
  }
  await visit(runtimeRoot);
  runtimeFiles.sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
  if (runtimeFiles.length === 0) throw new Error('PlayCanvas runtime tree is empty');
  const runtimeBytes = Buffer.from(`${JSON.stringify({
    schema: 'gsplat-playcanvas-runtime-tree/v1',
    root: 'node_modules/playcanvas/build/playcanvas',
    files: runtimeFiles
  }, null, 2)}\n`);
  const lockSource = resolve(harnessRoot, 'package-lock.json');
  const lockBytes = await readFile(lockSource);
  return { runtimeBytes, lockBytes, lockSource };
}

export async function materializeQ1BuildArtifacts(config, harnessRoot) {
  if (!config) return null;
  const buildRoot = resolve(config.artifactRoot, 'build');
  await mkdir(buildRoot);
  const { runtimeBytes, lockBytes, lockSource } = await q1BuildArtifactContents(harnessRoot);
  const runtimeDestination = resolve(buildRoot, 'playcanvas-runtime-tree.json');
  const lockDestination = resolve(buildRoot, 'package-lock.json');
  await writeFile(runtimeDestination, runtimeBytes);
  await copyFile(lockSource, lockDestination);
  return {
    runtime_js: {
      path: relative(config.seriesRoot, runtimeDestination).split('\\').join('/'),
      sha256: sha256(runtimeBytes)
    },
    package_lock: {
      path: relative(config.seriesRoot, lockDestination).split('\\').join('/'),
      sha256: sha256(lockBytes)
    }
  };
}

export async function verifyQ1BuildArtifacts(config, harnessRoot, artifacts) {
  if (!config) return;
  const { runtimeBytes, lockBytes } = await q1BuildArtifactContents(harnessRoot);
  const observed = {
    runtime_js: sha256(runtimeBytes),
    package_lock: sha256(lockBytes)
  };
  for (const [name, digest] of Object.entries(observed)) {
    if (artifacts?.[name]?.sha256 !== digest) {
      throw new Error(`Q1 ${name} changed during collection`);
    }
  }
}

export function q1EnvironmentFields(config, observed) {
  if (!config) return null;
  const adapterReceipt = normalizeQ1WebGpuEnvironmentReceipt(observed.adapterReceipt);
  const driverStack = observed.driverStack;
  if (driverStack?.source !== 'macos_sw_vers_buildVersion' ||
      driverStack.pre !== driverStack.post) {
    throw new Error('Q1 run lacks a stable observed Apple driver-stack identity');
  }
  const driver = `apple_metal_os_build:${requiredText(driverStack.post, 'Apple OS build')}`;
  const thermal = observed.thermal;
  if (!thermal || !['nominal', 'fair', 'moderate'].includes(thermal.pre) ||
      !['nominal', 'fair', 'moderate'].includes(thermal.post) ||
      thermal.admitted !== true) {
    throw new Error('Q1 run lacks an admissible observed thermal receipt');
  }
  requiredText(thermal.source, 'thermal source');
  const browserProcessArgs = observed.browserProcessArgsReceipt;
  if (browserProcessArgs?.schema !== Q1_BROWSER_PROCESS_ARGS_SCHEMA ||
      browserProcessArgs.source !== 'node_child_process_spawnargs' ||
      !Array.isArray(browserProcessArgs.normalized_args) ||
      browserProcessArgs.normalized_args.length === 0 ||
      browserProcessArgs.normalized_sha256 !==
        sha256(Buffer.from(JSON.stringify(browserProcessArgs.normalized_args)))) {
    throw new Error('Q1 run lacks a valid actual browser process argument receipt');
  }
  const adapterSupportedLimits = adapterReceipt.selected_adapter.supported_limits;
  const canonicalSupportedLimits = adapterReceipt.canonical_adapter.supported_limits;
  const deviceEffectiveLimits = adapterReceipt.selected_device.effective_limits;
  return {
    adapter: Q1_ADAPTER_SELECTION_CLASS,
    adapter_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
    driver,
    driver_source: driverStack.source,
    browser_executable_sha256: requiredSha(
      observed.browserExecutableSha256,
      'browser executable SHA-256'
    ),
    browser_launch_args_sha256: requiredSha(
      browserProcessArgs.normalized_sha256,
      'browser process arguments SHA-256'
    ),
    browser_launch_args_receipt: browserProcessArgs,
    canonical_adapter_supported_limits_sha256: sha256(
      Buffer.from(JSON.stringify(canonicalSupportedLimits))
    ),
    adapter_supported_limits_sha256: sha256(
      Buffer.from(JSON.stringify(adapterSupportedLimits))
    ),
    device_effective_limits_sha256: sha256(
      Buffer.from(JSON.stringify(deviceEffectiveLimits))
    ),
    webgpu_device_environment_receipt: adapterReceipt,
    power_source: requiredText(observed.powerSource, 'power source'),
    collection_session_id: config.collectionSessionId,
    thermal
  };
}

export function parseMacPowerReceipt(output) {
  const match = /Now drawing from '([^']+)'/.exec(output);
  if (!match) throw new Error('pmset did not report the active power source');
  return match[1].trim().toLowerCase().replaceAll(' ', '_');
}

export function parseMacThermalReceipt(output) {
  if (/No thermal warning level has been recorded/.test(output)) return 'nominal';
  throw new Error('macOS thermal state is not admissible or cannot be interpreted');
}
