import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';

const TEXT_MAX_BUFFER = 16 * 1024 * 1024;
const PNG_MAX_BUFFER = 64 * 1024 * 1024;
const RECEIPT_SCHEMA = 'gsplat-android-device-receipt/v1';

function requireText(value, label) {
  const result = value?.trim();
  if (!result) throw new Error(`${label} must be a non-empty string`);
  return result;
}

function requireSafeSerial(value) {
  const serial = requireText(value, 'PLAYCANVAS_ADB_SERIAL');
  if (serial.startsWith('-') || /[\s\x00-\x1f\x7f]/.test(serial)) {
    throw new Error('PLAYCANVAS_ADB_SERIAL contains forbidden characters');
  }
  return serial;
}

function parseProperties(output) {
  const properties = new Map();
  for (const line of output.split(/\r?\n/)) {
    const match = /^\[([^\]]+)\]: \[(.*)\]$/.exec(line.trim());
    if (match) properties.set(match[1], match[2]);
  }
  return properties;
}

function property(properties, name, { optional = false } = {}) {
  const value = properties.get(name)?.trim();
  if (!value && !optional) throw new Error(`adb getprop omitted ${name}`);
  return value || null;
}

function parseBooleanSignal(output, patterns, label) {
  const values = [];
  for (const pattern of patterns) {
    const match = pattern.exec(output);
    if (match) values.push(match[1].toLowerCase() === 'true');
  }
  if (values.length === 0) throw new Error(`adb receipt lacks ${label} evidence`);
  return values.some(Boolean);
}

function parseTopResumedComponent(output, hostPackage) {
  const line = output.split(/\r?\n/).find((candidate) =>
    /(?:topResumedActivity|mResumedActivity|ResumedActivity)/.test(candidate) &&
    candidate.includes(`${hostPackage}/`)
  );
  if (!line) {
    throw new Error(`Android runtime host package ${hostPackage} is not the top-resumed activity`);
  }
  const component = line.match(/([A-Za-z0-9._]+\/[A-Za-z0-9._$]+)/)?.[1] ?? null;
  if (!component || !component.startsWith(`${hostPackage}/`)) {
    throw new Error('cannot parse the top-resumed Android runtime host component');
  }
  return component;
}

function parsePackageVersion(output, packageName) {
  const versionName = output.match(/^\s*versionName=([^\s]+)\s*$/m)?.[1] ?? null;
  const versionCode = output.match(/^\s*versionCode=(\d+)/m)?.[1] ?? null;
  if (!versionName || !versionCode) {
    throw new Error(`cannot read ${packageName} package version from adb`);
  }
  return { versionName, versionCode };
}

function parseDisplayState(powerOutput, displayOutput) {
  const powerState = powerOutput.match(/\bDisplay Power:\s*state=([A-Za-z_]+)/i)?.[1] ??
    powerOutput.match(/\bmScreenOn=(true|false)\b/i)?.[1] ?? null;
  if (powerState) return { value: powerState, source: 'dumpsys_power' };

  const displayDeviceLine = displayOutput.split(/\r?\n/).find((line) =>
    line.includes('DisplayDeviceInfo{') && /\bstate\s+[A-Za-z_]+\b/i.test(line)
  );
  const displayState = displayDeviceLine?.match(/\bstate\s+([A-Za-z_]+)\b/i)?.[1] ??
    displayOutput.match(/^\s*mState=([A-Za-z_]+)\s*$/im)?.[1] ?? null;
  return { value: displayState, source: displayState ? 'dumpsys_display' : null };
}

async function runText(execFile, adbPath, args, label) {
  let result;
  try {
    result = await execFile(adbPath, args, {
      encoding: 'utf8',
      maxBuffer: TEXT_MAX_BUFFER
    });
  } catch (error) {
    throw new Error(`${label} failed: ${error?.message ?? String(error)}`, { cause: error });
  }
  return String(result?.stdout ?? '');
}

function requireForwardMapping(output, serial, port, cdpSocket) {
  const expectedLocal = `tcp:${port}`;
  const expectedRemote = `localabstract:${cdpSocket}`;
  const mapping = output.split(/\r?\n/).map((line) => line.trim()).find((line) => {
    const [mappedSerial, local, remote] = line.split(/\s+/);
    return mappedSerial === serial && local === expectedLocal && remote === expectedRemote;
  });
  if (!mapping) {
    throw new Error(
      `adb forward receipt does not bind ${serial} ${expectedLocal} to ${expectedRemote}`
    );
  }
  return { serial, local: expectedLocal, remote: expectedRemote };
}

function runtimeIdentity(receipt) {
  if (receipt?.browser_runtime) {
    return receipt.browser_runtime;
  }
  if (receipt?.chrome) {
    return {
      kind: 'chrome',
      cdp_socket: 'chrome_devtools_remote',
      host: receipt.chrome,
      engine: receipt.chrome
    };
  }
  throw new Error('Android device receipt omits browser runtime identity');
}

function parseRect(text, label) {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = text.match(new RegExp(`${escaped}=\\[(-?\\d+),(-?\\d+)\\]\\[(-?\\d+),(-?\\d+)\\]`));
  if (!match) return null;
  return {
    left: Number(match[1]),
    top: Number(match[2]),
    right: Number(match[3]),
    bottom: Number(match[4])
  };
}

function parseWindowPresentation(output, hostPackage) {
  const lines = output.split(/\r?\n/);
  const start = lines.findIndex((line) => /^\s*Window #\d+ /.test(line) && line.includes(`${hostPackage}/`));
  if (start < 0) throw new Error(`dumpsys window omits the ${hostPackage} host window`);
  let end = start + 1;
  while (end < lines.length && !/^\s*Window #\d+ /.test(lines[end])) end += 1;
  const block = lines.slice(start, end).join('\n');
  const requested = block.match(/\bRequested w=(\d+) h=(\d+)\b/);
  const frameLine = block.split(/\r?\n/).find((line) => /\bFrames:/.test(line)) ?? '';
  const surfaceShown = block.match(/\bSurface:\s+shown=(true|false)\b/)?.[1] ?? null;
  const obscured = block.match(/\bmObscured=(true|false)\b/)?.[1] ?? null;
  if (!requested || !surfaceShown || !obscured) {
    throw new Error(`dumpsys window host block for ${hostPackage} lacks presentation fields`);
  }
  return {
    source: 'adb_dumpsys_window_windows',
    host_package: hostPackage,
    requested_width: Number(requested[1]),
    requested_height: Number(requested[2]),
    parent_frame: parseRect(frameLine, 'parent'),
    display_frame: parseRect(frameLine, 'display'),
    window_frame: parseRect(frameLine, 'frame'),
    given_content_insets: parseRect(block, 'mGivenContentInsets'),
    given_visible_insets: parseRect(block, 'mGivenVisibleInsets'),
    surface_shown: surfaceShown === 'true',
    obscured: obscured === 'true'
  };
}

function rectEquals(rect, width, height) {
  return rect?.left === 0 && rect?.top === 0 && rect?.right === width && rect?.bottom === height;
}

function zeroInsets(rect) {
  return rect?.left === 0 && rect?.top === 0 && rect?.right === 0 && rect?.bottom === 0;
}

export function verifyAndroidWindowPresentation(receipt, expectedWidth, expectedHeight) {
  const window = receipt?.window_presentation;
  const valid = window?.requested_width === expectedWidth &&
    window?.requested_height === expectedHeight &&
    rectEquals(window?.parent_frame, expectedWidth, expectedHeight) &&
    rectEquals(window?.display_frame, expectedWidth, expectedHeight) &&
    rectEquals(window?.window_frame, expectedWidth, expectedHeight) &&
    zeroInsets(window?.given_content_insets) && zeroInsets(window?.given_visible_insets) &&
    window?.surface_shown === true && window?.obscured === false;
  if (!valid) {
    throw new Error(
      `Android host window is not an unobscured ${expectedWidth}x${expectedHeight} surface: ` +
      JSON.stringify(window)
    );
  }
  return { verified: true, expected_width: expectedWidth, expected_height: expectedHeight };
}

function stableDeviceIdentity(receipt) {
  const runtime = runtimeIdentity(receipt);
  return {
    serial: receipt.adb.serial,
    manufacturer: receipt.device.manufacturer,
    model: receipt.device.model,
    product_device: receipt.device.product_device,
    build_fingerprint: receipt.device.build_fingerprint,
    android_release: receipt.device.android_release,
    sdk: receipt.device.sdk,
    runtime_kind: runtime.kind,
    runtime_host_package: runtime.host.package,
    runtime_host_version_name: runtime.host.version_name,
    runtime_engine_package: runtime.engine.package,
    runtime_engine_version_name: runtime.engine.version_name
  };
}

export function androidRemoteConfig(environment = process.env) {
  const runtimeKind = environment.PLAYCANVAS_ANDROID_RUNTIME?.trim() || 'chrome';
  if (!['chrome', 'webview'].includes(runtimeKind)) {
    throw new Error('PLAYCANVAS_ANDROID_RUNTIME must be chrome or webview');
  }
  const chromePackage = environment.PLAYCANVAS_CHROME_PACKAGE?.trim() || 'com.android.chrome';
  const hostPackage = runtimeKind === 'webview'
    ? requireText(environment.PLAYCANVAS_ANDROID_HOST_PACKAGE, 'PLAYCANVAS_ANDROID_HOST_PACKAGE')
    : chromePackage;
  const runtimePackage = runtimeKind === 'webview'
    ? environment.PLAYCANVAS_ANDROID_WEBVIEW_PACKAGE?.trim() || 'com.google.android.webview'
    : chromePackage;
  const cdpSocket = runtimeKind === 'webview'
    ? requireText(environment.PLAYCANVAS_ANDROID_CDP_SOCKET, 'PLAYCANVAS_ANDROID_CDP_SOCKET')
    : 'chrome_devtools_remote';
  if (!/^[A-Za-z0-9._]+$/.test(hostPackage) || !/^[A-Za-z0-9._]+$/.test(runtimePackage) ||
      !/^[A-Za-z0-9._-]+$/.test(cdpSocket)) {
    throw new Error('Android runtime package or CDP socket contains forbidden characters');
  }
  return {
    adbPath: environment.ADB_PATH?.trim() || 'adb',
    adbSerial: requireSafeSerial(environment.PLAYCANVAS_ADB_SERIAL),
    androidRuntimeKind: runtimeKind,
    hostPackage,
    runtimePackage,
    cdpSocket,
    targetMode: runtimeKind === 'webview' ? 'existing-page' : 'new-page',
    expectedReceiptPath: environment.PLAYCANVAS_DEVICE_RECEIPT_PATH?.trim() || null
  };
}

export async function loadExpectedAndroidDeviceReceipt(
  path,
  { readFileImpl = readFile } = {}
) {
  if (!path) return null;
  let bytes;
  try {
    bytes = await readFileImpl(path);
  } catch (error) {
    throw new Error(`cannot read PLAYCANVAS_DEVICE_RECEIPT_PATH: ${error.message}`, {
      cause: error
    });
  }
  let receipt;
  try {
    receipt = JSON.parse(bytes.toString('utf8'));
  } catch (error) {
    throw new Error(`PLAYCANVAS_DEVICE_RECEIPT_PATH is not valid JSON: ${error.message}`, {
      cause: error
    });
  }
  if (receipt?.schema === 'gsplat-android-benchmark-device-evidence/v1') {
    receipt = receipt.post;
  }
  if (receipt?.schema !== RECEIPT_SCHEMA) {
    throw new Error(`device receipt schema must equal ${RECEIPT_SCHEMA}`);
  }
  return {
    path,
    sha256: createHash('sha256').update(bytes).digest('hex'),
    receipt
  };
}

export function verifyExpectedAndroidDeviceReceipt(expectedEnvelope, observed) {
  if (!expectedEnvelope) return null;
  const expected = stableDeviceIdentity(expectedEnvelope.receipt);
  const actual = stableDeviceIdentity(observed);
  const mismatch = Object.keys(actual).find((key) => expected[key] !== actual[key]);
  if (mismatch) {
    throw new Error(
      `device receipt identity mismatch for ${mismatch}: ` +
      `expected ${JSON.stringify(expected[mismatch])}, observed ${JSON.stringify(actual[mismatch])}`
    );
  }
  return {
    source_path: expectedEnvelope.path,
    sha256: expectedEnvelope.sha256,
    identity_match: true
  };
}

export function verifyStableAndroidDeviceIdentity(before, after) {
  const expected = stableDeviceIdentity(before);
  const actual = stableDeviceIdentity(after);
  const mismatch = Object.keys(actual).find((key) => expected[key] !== actual[key]);
  if (mismatch) {
    throw new Error(
      `Android device identity changed during the run for ${mismatch}: ` +
      `${JSON.stringify(expected[mismatch])} -> ${JSON.stringify(actual[mismatch])}`
    );
  }
  return { stable: true, identity: actual };
}

export async function collectAndroidDeviceReceipt({
  execFile,
  adbPath,
  serial,
  cdpPort,
  chromePackage,
  androidRuntimeKind = 'chrome',
  hostPackage = chromePackage,
  runtimePackage = chromePackage,
  cdpSocket = 'chrome_devtools_remote',
  phase,
  now = () => new Date().toISOString()
}) {
  const state = (await runText(
    execFile,
    adbPath,
    ['-s', serial, 'get-state'],
    'adb get-state'
  )).trim();
  if (state !== 'device') throw new Error(`adb ${serial} state is ${JSON.stringify(state)}, not device`);

  const observedSerial = (await runText(
    execFile,
    adbPath,
    ['-s', serial, 'get-serialno'],
    'adb get-serialno'
  )).trim();
  if (observedSerial !== serial) {
    throw new Error(`adb serial mismatch: requested ${serial}, observed ${observedSerial}`);
  }

  const forwardOutput = await runText(
    execFile,
    adbPath,
    ['forward', '--list'],
    'adb forward --list'
  );
  const forward = requireForwardMapping(forwardOutput, serial, cdpPort, cdpSocket);
  const properties = parseProperties(await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'getprop'],
    'adb getprop'
  ));
  const activityOutput = await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'dumpsys', 'activity', 'activities'],
    'adb dumpsys activity activities'
  );
  const policyOutput = await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'dumpsys', 'window', 'policy'],
    'adb dumpsys window policy'
  );
  const powerOutput = await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'dumpsys', 'power'],
    'adb dumpsys power'
  );
  const displayOutput = await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'dumpsys', 'display'],
    'adb dumpsys display'
  );
  const windowOutput = await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'dumpsys', 'window', 'windows'],
    'adb dumpsys window windows'
  );
  const hostPackageOutput = await runText(
    execFile,
    adbPath,
    ['-s', serial, 'shell', 'dumpsys', 'package', hostPackage],
    `adb dumpsys package ${hostPackage}`
  );
  const runtimePackageOutput = runtimePackage === hostPackage
    ? hostPackageOutput
    : await runText(
      execFile,
      adbPath,
      ['-s', serial, 'shell', 'dumpsys', 'package', runtimePackage],
      `adb dumpsys package ${runtimePackage}`
    );

  const keyguardLocked = parseBooleanSignal(policyOutput, [
    /\bmKeyguardShowing=(true|false)\b/i,
    /\bisStatusBarKeyguard=(true|false)\b/i,
    /^\s*showing=(true|false)\s*$/im,
    /^\s*showingAndNotOccluded=(true|false)\s*$/im
  ], 'keyguard state');
  if (keyguardLocked) throw new Error('Android device is locked; refusing to benchmark');

  const wakefulness = powerOutput.match(/\bmWakefulness=([A-Za-z_]+)/)?.[1] ??
    powerOutput.match(/\bWakefulness:\s*([A-Za-z_]+)/i)?.[1] ?? null;
  if (wakefulness?.toLowerCase() !== 'awake') {
    throw new Error(`Android device is not awake (wakefulness=${wakefulness ?? 'unknown'})`);
  }
  const displayStateReceipt = parseDisplayState(powerOutput, displayOutput);
  const displayState = displayStateReceipt.value;
  const displayOn = displayState?.toLowerCase() === 'on' || displayState?.toLowerCase() === 'true';
  if (!displayOn) {
    throw new Error(`Android display is not on (display_state=${displayState ?? 'unknown'})`);
  }

  const topResumedComponent = parseTopResumedComponent(activityOutput, hostPackage);
  const hostPackageVersion = parsePackageVersion(hostPackageOutput, hostPackage);
  const runtimePackageVersion = parsePackageVersion(runtimePackageOutput, runtimePackage);
  const windowPresentation = parseWindowPresentation(windowOutput, hostPackage);
  const hostIdentity = {
    package: hostPackage,
    version_name: hostPackageVersion.versionName,
    version_code: hostPackageVersion.versionCode
  };
  const engineIdentity = {
    package: runtimePackage,
    version_name: runtimePackageVersion.versionName,
    version_code: runtimePackageVersion.versionCode
  };
  return {
    schema: RECEIPT_SCHEMA,
    phase: requireText(phase, 'device receipt phase'),
    observed_at_utc: now(),
    adb: {
      serial,
      state,
      observed_serial: observedSerial,
      forward
    },
    device: {
      manufacturer: property(properties, 'ro.product.manufacturer'),
      model: property(properties, 'ro.product.model'),
      product_device: property(properties, 'ro.product.device'),
      build_fingerprint: property(properties, 'ro.build.fingerprint'),
      android_release: property(properties, 'ro.build.version.release'),
      sdk: property(properties, 'ro.build.version.sdk'),
      hardware: property(properties, 'ro.hardware', { optional: true }),
      soc_manufacturer: property(properties, 'ro.soc.manufacturer', { optional: true }),
      soc_model: property(properties, 'ro.soc.model', { optional: true })
    },
    state: {
      keyguard_locked: false,
      wakefulness,
      display_on: true,
      display_state: displayState,
      display_state_source: displayStateReceipt.source,
      runtime_host_top_resumed: true,
      chrome_top_resumed: androidRuntimeKind === 'chrome',
      top_resumed_component: topResumedComponent
    },
    window_presentation: windowPresentation,
    browser_runtime: {
      kind: androidRuntimeKind,
      cdp_socket: cdpSocket,
      host: hostIdentity,
      engine: engineIdentity
    },
    chrome: androidRuntimeKind === 'chrome' ? engineIdentity : null
  };
}

export function verifyAndroidBrowserIdentity(receipt, { browserVersion, userAgent, platform }) {
  const version = requireText(browserVersion, 'CDP browser version');
  const ua = requireText(userAgent, 'browser user agent');
  const navigatorPlatform = requireText(platform, 'browser platform');
  if (!/\bAndroid\b/i.test(ua) || !/\bLinux\b/i.test(navigatorPlatform)) {
    throw new Error(
      `remote CDP target is not an observed Android browser: ` +
      `platform=${JSON.stringify(navigatorPlatform)} userAgent=${JSON.stringify(ua)}`
    );
  }
  const cdpChromeVersion = version.match(/(?:Chrome|Chromium)\/([0-9.]+)/i)?.[1] ?? null;
  if (!cdpChromeVersion) throw new Error(`CDP browser is not Chrome/Chromium: ${version}`);
  const runtime = runtimeIdentity(receipt);
  const packageVersion = runtime.engine.version_name.match(/[0-9]+(?:\.[0-9]+)+/)?.[0] ?? null;
  if (!packageVersion || packageVersion !== cdpChromeVersion) {
    throw new Error(
      `CDP Chrome version ${cdpChromeVersion} does not match adb package ` +
      `${runtime.engine.package} version ${runtime.engine.version_name}`
    );
  }
  if (runtime.kind === 'webview' && !/\bwv\b/i.test(ua)) {
    throw new Error(`CDP target does not identify Android WebView: ${ua}`);
  }
  return {
    identity_match: true,
    cdp_browser_version: version,
    cdp_chrome_version: cdpChromeVersion,
    user_agent: ua,
    navigator_platform: navigatorPlatform,
    android_runtime_kind: runtime.kind,
    adb_runtime_host_package: runtime.host.package,
    adb_runtime_host_version_name: runtime.host.version_name,
    adb_runtime_engine_package: runtime.engine.package,
    adb_runtime_engine_version_name: runtime.engine.version_name
  };
}

export function parsePngDimensions(bytes) {
  const buffer = Buffer.from(bytes);
  const signature = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  if (buffer.length < 24 || !buffer.subarray(0, 8).equals(signature) ||
      buffer.toString('ascii', 12, 16) !== 'IHDR') {
    throw new Error('adb screencap did not return a valid PNG IHDR');
  }
  const width = buffer.readUInt32BE(16);
  const height = buffer.readUInt32BE(20);
  if (width <= 0 || height <= 0) throw new Error('adb screencap PNG dimensions are invalid');
  return { width, height };
}

export async function captureAndroidScreenReceipt({
  execFile,
  adbPath,
  serial,
  expectedWidth,
  expectedHeight,
  now = () => new Date().toISOString()
}) {
  let result;
  try {
    result = await execFile(adbPath, ['-s', serial, 'exec-out', 'screencap', '-p'], {
      encoding: 'buffer',
      maxBuffer: PNG_MAX_BUFFER
    });
  } catch (error) {
    throw new Error(`read-only adb screencap failed: ${error?.message ?? String(error)}`, {
      cause: error
    });
  }
  const png = Buffer.from(result?.stdout ?? []);
  const { width, height } = parsePngDimensions(png);
  if (width !== expectedWidth || height !== expectedHeight) {
    throw new Error(
      `device screen receipt mismatch: expected ${expectedWidth}x${expectedHeight}, ` +
      `observed ${width}x${height}`
    );
  }
  return {
    png,
    receipt: {
      source: 'adb_exec_out_screencap_png_ihdr',
      command_effect: 'read_only',
      serial,
      captured_at_utc: now(),
      width,
      height,
      byte_count: png.length,
      sha256: createHash('sha256').update(png).digest('hex')
    }
  };
}

export { RECEIPT_SCHEMA as ANDROID_DEVICE_RECEIPT_SCHEMA };
