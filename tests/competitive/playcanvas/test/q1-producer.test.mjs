import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import {
  Q1_PRODUCER_REQUEST_SCHEMA,
  Q1_QUALITY_ONLY_REQUEST_SCHEMA,
  assertQ1Invocation,
  loadQ1ProducerConfig,
  materializeQ1BuildArtifacts,
  parseMacPowerReceipt,
  parseMacThermalReceipt,
  q1BrowserProcessArgsReceipt,
  q1EnvironmentFields,
  q1ManifestFields,
  q1PairingFields,
  q1QualityOnlyManifestFields,
  q1RendererCaptureEnabled,
  validateQ1QualityOnlyArtifact,
  validateQ1ProducerRequest,
  verifyQ1BuildArtifacts
} from '../scripts/q1-producer.mjs';
import {
  Q1_ADAPTER_IDENTITY_STATUS,
  Q1_ADAPTER_SELECTION_CLASS,
  Q1_CANONICAL_ADAPTER_SCHEMA,
  Q1_CANONICAL_SUPPORTED_LIMIT_NAMES,
  Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
  Q1_WEBGPU_ENVIRONMENT_SCHEMA
} from '../public/webgpu-environment-receipt.js';

const SHA_A = 'a'.repeat(64);
const SHA_B = 'b'.repeat(64);
const SHA_C = 'c'.repeat(64);
const SHA_D = 'd'.repeat(64);

function supportedLimits(offset = 0) {
  return Object.fromEntries(
    Q1_CANONICAL_SUPPORTED_LIMIT_NAMES.map((name, index) => [name, 1000 + index + offset])
  );
}

function adapterReceipt(overrides = {}) {
  const limits = supportedLimits();
  return {
    schema: Q1_WEBGPU_ENVIRONMENT_SCHEMA,
    endpoint: 'playcanvas',
    selected_adapter: {
      provenance: 'playcanvas_graphicsDevice.gpuAdapter',
      info_status: 'browser_exposed',
      info: {
        vendor: 'Apple',
        architecture: 'apple8',
        device: 'M4',
        description: 'Metal',
        subgroupMinSize: 4,
        subgroupMaxSize: 32
      },
      supported_limits: limits
    },
    selected_device: {
      provenance: 'playcanvas_graphicsDevice.wgpu',
      effective_limits: supportedLimits(-100)
    },
    canonical_adapter: {
      schema: Q1_CANONICAL_ADAPTER_SCHEMA,
      selection_class: Q1_ADAPTER_SELECTION_CLASS,
      backend_class: 'browser_webgpu',
      hardware_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
      supported_limits_schema: Q1_CANONICAL_SUPPORTED_LIMITS_SCHEMA,
      supported_limits: limits
    },
    ...overrides
  };
}

function request(role, overrides = {}) {
  const base = {
    schema: Q1_PRODUCER_REQUEST_SCHEMA,
    artifact_role: role,
    series_id: 'q1-series-001',
    schedule_sha256: SHA_A,
    protocol_sha256: SHA_B,
    configuration_sha256: SHA_C,
    pair_id: 'pair-01',
    run_order: 'playcanvas-first',
    position: 1,
    collection_session_id: 'm4-chrome-session-001'
  };
  if (role === 'control') {
    base.capture_trace_frame_index = 0;
  } else {
    base.control_bindings = [
      {
        trace_frame_index: 1,
        run_id: 'control-1',
        manifest_sha256: SHA_D,
        configuration_sha256: SHA_C
      },
      {
        trace_frame_index: 0,
        run_id: 'control-0',
        manifest_sha256: SHA_A,
        configuration_sha256: SHA_C
      }
    ];
  }
  return { ...base, ...overrides };
}

function qualityOnlyRequest(overrides = {}) {
  return {
    schema: Q1_QUALITY_ONLY_REQUEST_SCHEMA,
    artifact_role: 'quality_only',
    formal_view_id: '000001',
    trace_frame_index: 0,
    protocol_sha256: SHA_B,
    collection_session_id: 'm4-chrome-quality-001',
    product_quality_state: 'Deferred',
    performance_authorized: false,
    ...overrides
  };
}

function invocation(captureTraceFrame) {
  return {
    qualificationName: 'truck-quality-1080p-v1',
    cameraMode: 'sequence',
    warmupFrames: 20,
    measuredFrames: 80,
    captureTraceFrame,
    viewportWidth: 1920,
    viewportHeight: 1080,
    sessionMode: 'local-launch',
    headless: false
  };
}

function queueTerminal() {
  return {
    sustained: {
      measuredFrameCount: 80,
      submitVersionStart: 100,
      submitVersionEnd: 180,
      queueTerminalSpanMs: 1600
    },
    measurementDrain: {
      endedAtMs: 5000,
      submitVersionStable: true
    }
  };
}

test('control and throughput roles remain separate and invocation-locked', () => {
  const control = validateQ1ProducerRequest(request('control'));
  assert.equal(q1RendererCaptureEnabled(control), true);
  assert.doesNotThrow(() => assertQ1Invocation(control, invocation(0)));
  assert.deepEqual(q1ManifestFields(control, queueTerminal(), 80, 80), {
    artifact_role: 'control',
    protocol_sha256: SHA_B,
    configuration_sha256: SHA_C,
    performance_evidence: false,
    count_scope: 'full_membership_v_c_d_unavailable',
    capture_trace_frame_index: 0
  });

  const throughput = validateQ1ProducerRequest(request('throughput'));
  assert.equal(q1RendererCaptureEnabled(throughput), false);
  assert.doesNotThrow(() => assertQ1Invocation(throughput, invocation(null)));
  assert.deepEqual(throughput.controlBindings.map((value) => value.trace_frame_index), [0, 1]);
  const fields = q1ManifestFields(throughput, queueTerminal(), 80, 0);
  assert.equal(fields.performance_evidence, true);
  assert.deepEqual(fields.control_bindings, throughput.controlBindings);
  assert.deepEqual(fields.terminal_window, {
    schema: 'gsplat-q1-webgpu-terminal-window/v1',
    clock: 'performance_now_monotonic',
    start_boundary: 'first_measured_camera_input_accepted',
    end_boundary: 'final_measured_gpu_queue_completion',
    completion_primitive: 'gpu_queue_on_submitted_work_done',
    frame_loop_policy: 'controlled_presented_raf',
    camera_mutation_point: 'before_update_order_project_render',
    warmup_queue_drained: true,
    continuous_submissions: true,
    per_frame_observer_reads: 0,
    extra_submissions_during_terminal_drain: 0,
    measured_camera_input_count: 80,
    measured_submission_count: 80,
    dropped_frame_count: 0,
    submission_counter_stable_during_drain: true,
    submission_counter_before_first: 100,
    submission_counter_after_last: 180,
    started_at_monotonic_ms: 3400,
    completed_at_monotonic_ms: 5000,
    duration_ms: 1600
  });
  assert.throws(
    () => q1ManifestFields(
      throughput,
      {
        ...queueTerminal(),
        sustained: { ...queueTerminal().sustained, submitVersionEnd: 181 }
      },
      80,
      0
    ),
    /one continuous submission per measured frame/
  );
  assert.throws(
    () => q1ManifestFields(throughput, queueTerminal(), 80, 1),
    /one continuous submission per measured frame/
  );
});

test('quality-only request is independently locked and has no performance pairing', () => {
  const quality = validateQ1ProducerRequest(qualityOnlyRequest());
  assert.equal(q1RendererCaptureEnabled(quality), true);
  assert.equal(q1PairingFields(quality), null);
  assert.doesNotThrow(() => assertQ1Invocation(quality, {
    qualificationName: 'truck-formal-quality-979x546-v1',
    cameraMode: 'static',
    warmupFrames: 0,
    measuredFrames: 0,
    captureTraceFrame: 0,
    viewportWidth: 979,
    viewportHeight: 546,
    sessionMode: 'local-launch',
    headless: false
  }));
  assert.deepEqual(q1QualityOnlyManifestFields(quality), {
    artifact_role: 'quality_only',
    formal_view_id: '000001',
    trace_frame_index: 0,
    protocol_sha256: SHA_B,
    product_quality_state: 'Deferred',
    performance_authorized: false
  });
  assert.throws(
    () => validateQ1ProducerRequest(qualityOnlyRequest({ pairing: {} })),
    /fields must be exact/
  );
  assert.throws(
    () => validateQ1ProducerRequest(qualityOnlyRequest({ performance_authorized: true })),
    /disabled performance/
  );
  assert.throws(
    () => assertQ1Invocation(quality, {
      qualificationName: 'truck-formal-quality-979x546-v1',
      cameraMode: 'static',
      warmupFrames: 0,
      measuredFrames: 1,
      captureTraceFrame: 0,
      viewportWidth: 979,
      viewportHeight: 546,
      sessionMode: 'local-launch',
      headless: false
    }),
    /measuredFrames must equal 0/
  );
});

test('quality-only manifest fails closed on any benchmark result surface', () => {
  const rendererCapture = { status: 'terminal' };
  const splatCount = 2_541_226;
  const manifest = {
    dataset: {
      id: 'inria-3dgs-truck-iteration-30000',
      sha256: '65ecf4058135a030cddd2198326f67172a4101344b0b54a3fa370cf45ea9688c',
      splat_count: splatCount,
      sh_degree: 3
    },
    trace: {
      id: 'formal-truck-product-quality-000001-000009-979x546-v1',
      sha256: '46819f71d5025bb61f6583392448d977051a0b4c67a0c05db860232033c0676c',
      capture_frame_index: 0,
      formal_view_id: '000001'
    },
    exactness: {
      source_splat_count: splatCount,
      decoded_splat_count: splatCount,
      encoded_splat_count: splatCount,
      resident_splat_count: splatCount,
      addressable_splat_count: splatCount,
      source_sh_degree: 3,
      resident_sh_degree: 3,
      source_membership: 'all',
      sampling: 'disabled',
      lod: 'disabled',
      partial_scene_published: false,
      full_quality: true
    },
    resolution: {
      requested_width: 979,
      requested_height: 546,
      surface_width: 979,
      surface_height: 546,
      internal_render_width: 979,
      internal_render_height: 546,
      presented_width: 979,
      presented_height: 546,
      dynamic_resolution: 'disabled',
      upscaling: 'disabled',
      internal_full_resolution: true,
      full_resolution: true
    },
    presentation_capture: {
      excluded_from_performance: true,
      capture_trace_frame_index: 0,
      renderer_capture: rendererCapture,
      frames: [{ submit_version_before: 1, submit_version_after: 2 }]
    },
    renderer_capture: rendererCapture,
    product_quality: { state: 'Deferred' },
    performance_authorized: false,
    q1_quality: {
      artifact_role: 'quality_only',
      formal_view_id: '000001',
      performance_authorized: false
    }
  };
  assert.equal(validateQ1QualityOnlyArtifact(manifest), manifest);
  for (const field of ['timing', 'pairing', 'frames', 'sustained_throughput']) {
    assert.throws(
      () => validateQ1QualityOnlyArtifact({ ...manifest, [field]: {} }),
      new RegExp(`must not contain ${field}`)
    );
  }
  assert.throws(
    () => validateQ1QualityOnlyArtifact({ ...manifest, performance_authorized: true }),
    /frozen non-performance quality identity/
  );
  assert.throws(
    () => validateQ1QualityOnlyArtifact({
      ...manifest,
      environment: { mean_fps: 60 }
    }),
    /forbidden mean_fps/
  );
  assert.throws(
    () => validateQ1QualityOnlyArtifact({
      ...manifest,
      browser_presentation: { timing: {} }
    }),
    /forbidden timing/
  );
});

test('producer request rejects partial controls and mixed configurations', () => {
  assert.throws(
    () => validateQ1ProducerRequest(request('throughput', { control_bindings: [] })),
    /exactly two controls/
  );
  assert.throws(
    () => validateQ1ProducerRequest(request('throughput', {
      control_bindings: [
        request('throughput').control_bindings[0],
        { ...request('throughput').control_bindings[0], run_id: 'duplicate-trace' }
      ]
    })),
    /traces 0 and 1 exactly once/
  );
  assert.throws(
    () => validateQ1ProducerRequest(request('throughput', {
      control_bindings: request('throughput').control_bindings.map((binding, index) =>
        index === 0 ? { ...binding, configuration_sha256: SHA_D } : binding)
    })),
    /configuration does not match/
  );
  assert.throws(
    () => validateQ1ProducerRequest(request('control', { control_bindings: [] })),
    /must not contain/
  );
  assert.throws(
    () => assertQ1Invocation(
      validateQ1ProducerRequest(request('control')),
      { ...invocation(0), measuredFrames: 81 }
    ),
    /measuredFrames must equal 80/
  );
  assert.throws(
    () => assertQ1Invocation(
      validateQ1ProducerRequest(request('control')),
      { ...invocation(0), headless: true }
    ),
    /headless must equal false/
  );
});

test('request loading requires a fresh artifact child of its series', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gsplat-q1-request-'));
  try {
    const seriesRoot = resolve(root, 'series');
    const artifactRoot = resolve(seriesRoot, 'pair-01', 'playcanvas', 'control-0');
    const requestPath = resolve(root, 'request.json');
    await mkdir(seriesRoot);
    await writeFile(requestPath, JSON.stringify(request('control')));
    const config = await loadQ1ProducerConfig({
      PLAYCANVAS_Q1_PRODUCER_REQUEST: requestPath,
      PLAYCANVAS_Q1_SERIES_ROOT: seriesRoot
    }, artifactRoot);
    assert.equal(config.artifactRoot, artifactRoot);
    assert.equal(config.seriesRoot, seriesRoot);
    assert.deepEqual(q1PairingFields(config), {
      series_id: 'q1-series-001',
      schedule_sha256: SHA_A,
      pair_id: 'pair-01',
      run_order: 'playcanvas-first',
      position: 1,
      fresh_output: true,
      automatic_retry: false
    });
    await assert.rejects(
      loadQ1ProducerConfig({
        PLAYCANVAS_Q1_PRODUCER_REQUEST: requestPath,
        PLAYCANVAS_Q1_SERIES_ROOT: seriesRoot
      }, resolve(root, 'outside')),
      /inside the Q1 series root/
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('build artifacts are immutable content-addressed copies inside the series', async () => {
  const root = await mkdtemp(join(tmpdir(), 'gsplat-q1-build-'));
  try {
    const seriesRoot = resolve(root, 'series');
    const artifactRoot = resolve(seriesRoot, 'pair-01', 'playcanvas', 'throughput');
    const harnessRoot = resolve(root, 'harness');
    await mkdir(artifactRoot, { recursive: true });
    await mkdir(
      resolve(harnessRoot, 'node_modules/playcanvas/build/playcanvas/src'),
      { recursive: true }
    );
    await writeFile(
      resolve(harnessRoot, 'node_modules/playcanvas/build/playcanvas/src/index.js'),
      'runtime bytes\n'
    );
    await writeFile(resolve(harnessRoot, 'package-lock.json'), '{"lock":true}\n');
    const artifacts = await materializeQ1BuildArtifacts(
      { seriesRoot, artifactRoot },
      harnessRoot
    );
    assert.deepEqual(Object.keys(artifacts).sort(), ['package_lock', 'runtime_js']);
    const runtimeTree = JSON.parse(
      await readFile(resolve(seriesRoot, artifacts.runtime_js.path), 'utf8')
    );
    assert.equal(runtimeTree.schema, 'gsplat-playcanvas-runtime-tree/v1');
    assert.deepEqual(runtimeTree.files.map((value) => value.path), ['src/index.js']);
    assert.match(artifacts.runtime_js.sha256, /^[0-9a-f]{64}$/);
    await assert.doesNotReject(
      verifyQ1BuildArtifacts({ seriesRoot, artifactRoot }, harnessRoot, artifacts)
    );
    await writeFile(
      resolve(harnessRoot, 'node_modules/playcanvas/build/playcanvas/src/index.js'),
      'changed runtime bytes\n'
    );
    await assert.rejects(
      verifyQ1BuildArtifacts({ seriesRoot, artifactRoot }, harnessRoot, artifacts),
      /runtime_js changed during collection/
    );
    await assert.rejects(
      materializeQ1BuildArtifacts({ seriesRoot, artifactRoot }, harnessRoot),
      /EEXIST/
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('environment receipt uses observed adapter, limits, host and thermal evidence', () => {
  const config = validateQ1ProducerRequest(request('control'));
  const processArgs = q1BrowserProcessArgsReceipt({
    spawnfile: '/Applications/Google Chrome',
    spawnargs: [
      '/Applications/Google Chrome',
      '--enable-unsafe-webgpu',
      '--enable-gpu',
      '--ignore-gpu-blocklist',
      '--remote-debugging-port=49152',
      '--user-data-dir=/private/tmp/profile-a'
    ],
    expectedExecutable: '/Applications/Google Chrome',
    requiredArgs: ['--enable-unsafe-webgpu', '--enable-gpu', '--ignore-gpu-blocklist']
  });
  const environment = q1EnvironmentFields(config, {
    adapterReceipt: adapterReceipt(),
    browserExecutableSha256: SHA_A,
    browserProcessArgsReceipt: processArgs,
    powerSource: 'ac_power',
    driverStack: {
      source: 'macos_sw_vers_buildVersion',
      pre: '25A1',
      post: '25A1'
    },
    thermal: { source: 'macos_pmset_thermal_warning_level', pre: 'nominal', post: 'fair', admitted: true }
  });
  assert.deepEqual({
    ...environment,
    canonical_adapter_supported_limits_sha256: '<canonical>',
    adapter_supported_limits_sha256: '<adapter>',
    device_effective_limits_sha256: '<device>',
    webgpu_device_environment_receipt: '<receipt>'
  }, {
    adapter: Q1_ADAPTER_SELECTION_CLASS,
    adapter_identity_status: Q1_ADAPTER_IDENTITY_STATUS,
    driver: 'apple_metal_os_build:25A1',
    driver_source: 'macos_sw_vers_buildVersion',
    browser_executable_sha256: SHA_A,
    browser_launch_args_sha256: processArgs.normalized_sha256,
    browser_launch_args_receipt: processArgs,
    canonical_adapter_supported_limits_sha256: '<canonical>',
    adapter_supported_limits_sha256: '<adapter>',
    device_effective_limits_sha256: '<device>',
    webgpu_device_environment_receipt: '<receipt>',
    power_source: 'ac_power',
    collection_session_id: 'm4-chrome-session-001',
    thermal: { source: 'macos_pmset_thermal_warning_level', pre: 'nominal', post: 'fair', admitted: true }
  });
  assert.match(environment.canonical_adapter_supported_limits_sha256, /^[0-9a-f]{64}$/);
  assert.match(environment.adapter_supported_limits_sha256, /^[0-9a-f]{64}$/);
  assert.match(environment.device_effective_limits_sha256, /^[0-9a-f]{64}$/);
  assert.deepEqual(environment.webgpu_device_environment_receipt, adapterReceipt());
  assert.throws(
    () => q1EnvironmentFields(config, {
      adapterReceipt: { schema: 'wrong' },
      thermal: { pre: 'nominal', post: 'nominal' }
    }),
    /renderer-selected WebGPU/
  );
  assert.throws(
    () => q1EnvironmentFields(config, {
      adapterReceipt: adapterReceipt({
        canonical_adapter: {
          ...adapterReceipt().canonical_adapter,
          supported_limits: {
            ...adapterReceipt().canonical_adapter.supported_limits,
            maxBufferSize: 9999
          }
        }
      }),
      browserExecutableSha256: SHA_A,
      browserProcessArgsReceipt: processArgs,
      powerSource: 'ac_power',
      driverStack: {
        source: 'macos_sw_vers_buildVersion',
        pre: '25A1',
        post: '25A1'
      },
      thermal: { source: 'observed', pre: 'nominal', post: 'nominal', admitted: true }
    }),
    /canonical adapter supported limit maxBufferSize is not selected-adapter data/
  );
  assert.throws(
    () => q1EnvironmentFields(config, {
      adapterReceipt: adapterReceipt(),
      browserExecutableSha256: SHA_A,
      browserProcessArgsReceipt: processArgs,
      powerSource: 'ac_power',
      driverStack: {
        source: 'macos_sw_vers_buildVersion',
        pre: '25A1',
        post: '25A1'
      },
      thermal: { source: 'observed', pre: 'nominal', post: 'nominal', admitted: false }
    }),
    /admissible observed thermal/
  );
  assert.throws(
    () => q1EnvironmentFields(config, {
      adapterReceipt: adapterReceipt(),
      driverStack: {
        source: 'macos_sw_vers_buildVersion',
        pre: '25A1',
        post: '25A2'
      }
    }),
    /stable observed Apple driver-stack identity/
  );
  assert.equal(parseMacPowerReceipt("Now drawing from 'AC Power'\n"), 'ac_power');
  assert.equal(
    parseMacThermalReceipt('Note: No thermal warning level has been recorded\n'),
    'nominal'
  );
  assert.throws(() => parseMacThermalReceipt('CPU_Speed_Limit = 50'), /not admissible/);
});

test('browser process argument receipt hashes actual argv with only run-variant values normalized', () => {
  const requiredArgs = ['--enable-gpu'];
  const first = q1BrowserProcessArgsReceipt({
    spawnfile: '/Applications/Chrome',
    spawnargs: [
      '/Applications/Chrome',
      '--enable-gpu',
      '--remote-debugging-port=50123',
      '--user-data-dir=/tmp/profile-one',
      '--disable-background-networking'
    ],
    expectedExecutable: '/Applications/Chrome',
    requiredArgs
  });
  const second = q1BrowserProcessArgsReceipt({
    spawnfile: '/Applications/Chrome',
    spawnargs: [
      '/Applications/Chrome',
      '--enable-gpu',
      '--remote-debugging-port=60124',
      '--user-data-dir=/tmp/profile-two',
      '--disable-background-networking'
    ],
    expectedExecutable: '/Applications/Chrome',
    requiredArgs
  });
  assert.equal(first.normalized_sha256, second.normalized_sha256);
  assert.deepEqual(first.normalized_args, [
    '<browser-executable>',
    '--enable-gpu',
    '--remote-debugging-port=<ephemeral-port>',
    '--user-data-dir=<ephemeral-profile>',
    '--disable-background-networking'
  ]);
  assert.throws(
    () => q1BrowserProcessArgsReceipt({
      spawnfile: '/Applications/Chrome',
      spawnargs: ['/Applications/Chrome', '--headless=new', '--enable-gpu'],
      expectedExecutable: '/Applications/Chrome',
      requiredArgs
    }),
    /headful launch configuration/
  );
});
