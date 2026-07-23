import assert from 'node:assert/strict';
import test from 'node:test';
import {
  ANDROID_DEVICE_RECEIPT_SCHEMA,
  androidRemoteConfig,
  captureAndroidScreenReceipt,
  collectAndroidDeviceReceipt,
  loadExpectedAndroidDeviceReceipt,
  parsePngDimensions,
  verifyAndroidBrowserIdentity,
  verifyAndroidWindowPresentation,
  verifyExpectedAndroidDeviceReceipt,
  verifyStableAndroidDeviceIdentity
} from '../scripts/android-device-receipt.mjs';

const serial = '033ed212';
const getprop = [
  '[ro.product.manufacturer]: [Nothing]',
  '[ro.product.model]: [A065]',
  '[ro.product.device]: [A065]',
  '[ro.build.fingerprint]: [Nothing/A065/build:16/test:user/release-keys]',
  '[ro.build.version.release]: [16]',
  '[ro.build.version.sdk]: [36]',
  '[ro.hardware]: [qcom]',
  '[ro.soc.manufacturer]: [Qualcomm]',
  '[ro.soc.model]: [SM_TEST]'
].join('\n');

function fixtureExec({ locked = false, oemPowerFormat = false, runtime = 'chrome' } = {}) {
  const webview = runtime === 'webview';
  const hostPackage = webview ? 'com.gsplat.competitive.playcanvas' : 'com.android.chrome';
  const component = webview
    ? 'com.gsplat.competitive.playcanvas/.MainActivity'
    : 'com.android.chrome/org.chromium.chrome.browser.ChromeTabbedActivity';
  const socket = webview ? 'webview_devtools_remote_17711' : 'chrome_devtools_remote';
  const calls = [];
  const execFile = async (program, args, options) => {
    calls.push({ program, args, options });
    const command = args.join(' ');
    if (command.endsWith('get-state')) return { stdout: 'device\n' };
    if (command.endsWith('get-serialno')) return { stdout: `${serial}\n` };
    if (command === 'forward --list') {
      return { stdout: `${serial} tcp:9222 localabstract:${socket}\n` };
    }
    if (command.endsWith('shell getprop')) return { stdout: getprop };
    if (command.endsWith('shell dumpsys activity activities')) {
      return {
        stdout:
          'topResumedActivity=ActivityRecord{123 u0 ' +
          `${component} t42}`
      };
    }
    if (command.endsWith('shell dumpsys window policy')) {
      return { stdout: `mKeyguardShowing=${locked ? 'true' : 'false'}\n` };
    }
    if (command.endsWith('shell dumpsys power')) {
      return {
        stdout: oemPowerFormat
          ? 'mWakefulness=Awake\nDisplay Power: com.android.server.power.PowerManagerService$1@abc\n'
          : 'mWakefulness=Awake\nDisplay Power: state=ON\n'
      };
    }
    if (command.endsWith('shell dumpsys display')) {
      return {
        stdout: 'DisplayDeviceInfo{"Built-in Screen", 1080 x 2412, state ON, committedState ON}'
      };
    }
    if (command.endsWith('shell dumpsys window windows')) {
      return {
        stdout: [
          `  Window #10 Window{abc u0 ${component}}:`,
          '    mObscured=false',
          '    Requested w=2412 h=1080 mLayoutSeq=42',
          '    mGivenContentInsets=[0,0][0,0] mGivenVisibleInsets=[0,0][0,0]',
          '    Frames: parent=[0,0][2412,1080] display=[0,0][2412,1080] frame=[0,0][2412,1080]',
          '      Surface: shown=true'
        ].join('\n')
      };
    }
    if (command.endsWith('shell dumpsys package com.gsplat.competitive.playcanvas')) {
      return { stdout: 'versionCode=1 minSdk=29\nversionName=1.0\n' };
    }
    if (command.endsWith('shell dumpsys package com.google.android.webview')) {
      return { stdout: 'versionCode=787104603 minSdk=32\nversionName=150.0.7871.46\n' };
    }
    if (command.endsWith('shell dumpsys package com.android.chrome')) {
      return { stdout: 'versionCode=123456 minSdk=29\nversionName=150.0.7871.130\n' };
    }
    throw new Error(`unexpected adb command: ${command}`);
  };
  return { execFile, calls };
}

async function receipt(options = {}) {
  const fixture = fixtureExec(options);
  const webview = options.runtime === 'webview';
  return {
    value: await collectAndroidDeviceReceipt({
      execFile: fixture.execFile,
      adbPath: 'adb',
      serial,
      cdpPort: 9222,
      androidRuntimeKind: webview ? 'webview' : 'chrome',
      hostPackage: webview ? 'com.gsplat.competitive.playcanvas' : 'com.android.chrome',
      runtimePackage: webview ? 'com.google.android.webview' : 'com.android.chrome',
      cdpSocket: webview ? 'webview_devtools_remote_17711' : 'chrome_devtools_remote',
      phase: 'test',
      now: () => '2026-07-23T00:00:00.000Z'
    }),
    calls: fixture.calls
  };
}

test('remote config requires an explicit safe ADB serial', () => {
  assert.throws(() => androidRemoteConfig({}), /PLAYCANVAS_ADB_SERIAL/);
  assert.throws(
    () => androidRemoteConfig({ PLAYCANVAS_ADB_SERIAL: '-d' }),
    /forbidden characters/
  );
  assert.deepEqual(androidRemoteConfig({
    PLAYCANVAS_ADB_SERIAL: serial,
    PLAYCANVAS_DEVICE_RECEIPT_PATH: '/tmp/device.json'
  }), {
    adbPath: 'adb',
    adbSerial: serial,
    androidRuntimeKind: 'chrome',
    hostPackage: 'com.android.chrome',
    runtimePackage: 'com.android.chrome',
    cdpSocket: 'chrome_devtools_remote',
    targetMode: 'new-page',
    expectedReceiptPath: '/tmp/device.json'
  });
  assert.deepEqual(androidRemoteConfig({
    PLAYCANVAS_ADB_SERIAL: serial,
    PLAYCANVAS_ANDROID_RUNTIME: 'webview',
    PLAYCANVAS_ANDROID_HOST_PACKAGE: 'com.gsplat.competitive.playcanvas',
    PLAYCANVAS_ANDROID_CDP_SOCKET: 'webview_devtools_remote_17711'
  }), {
    adbPath: 'adb',
    adbSerial: serial,
    androidRuntimeKind: 'webview',
    hostPackage: 'com.gsplat.competitive.playcanvas',
    runtimePackage: 'com.google.android.webview',
    cdpSocket: 'webview_devtools_remote_17711',
    targetMode: 'existing-page',
    expectedReceiptPath: null
  });
});

test('ADB receipt binds serial, forward, properties, unlocked state, and top Chrome', async () => {
  const { value, calls } = await receipt();
  assert.equal(value.schema, ANDROID_DEVICE_RECEIPT_SCHEMA);
  assert.equal(value.adb.serial, serial);
  assert.equal(value.adb.forward.local, 'tcp:9222');
  assert.equal(value.device.model, 'A065');
  assert.equal(value.device.soc_model, 'SM_TEST');
  assert.equal(value.state.keyguard_locked, false);
  assert.equal(value.state.chrome_top_resumed, true);
  assert.equal(value.state.runtime_host_top_resumed, true);
  assert.equal(value.chrome.version_name, '150.0.7871.130');
  assert.equal(value.browser_runtime.kind, 'chrome');
  assert.deepEqual(verifyAndroidWindowPresentation(value, 2412, 1080), {
    verified: true,
    expected_width: 2412,
    expected_height: 1080
  });
  assert.deepEqual(value.window_presentation.given_content_insets, {
    left: 0, top: 0, right: 0, bottom: 0
  });
  assert.ok(calls.every(({ args }) =>
    !args.some((item) => /^(?:install|uninstall|push|pull|input|am|pm)$/.test(item))
  ));
});

test('ADB receipt fails closed for a locked device', async () => {
  const fixture = fixtureExec({ locked: true });
  await assert.rejects(
    collectAndroidDeviceReceipt({
      execFile: fixture.execFile,
      adbPath: 'adb',
      serial,
      cdpPort: 9222,
      chromePackage: 'com.android.chrome',
      phase: 'test'
    }),
    /device is locked/
  );
});

test('ADB receipt accepts OEM power output only when dumpsys display proves ON', async () => {
  const { value } = await receipt({ oemPowerFormat: true });
  assert.equal(value.state.display_on, true);
  assert.equal(value.state.display_state, 'ON');
  assert.equal(value.state.display_state_source, 'dumpsys_display');
});

test('browser identity must be Android Chrome matching the top-resumed package', async () => {
  const { value } = await receipt();
  const matched = verifyAndroidBrowserIdentity(value, {
    browserVersion: 'Chrome/150.0.7871.130',
    userAgent: 'Mozilla/5.0 (Linux; Android 16; A065) Chrome/150.0.7871.130',
    platform: 'Linux armv8l'
  });
  assert.equal(matched.identity_match, true);
  assert.throws(
    () => verifyAndroidBrowserIdentity(value, {
      browserVersion: 'Chrome/150.0.7871.130',
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X)',
      platform: 'MacIntel'
    }),
    /not an observed Android browser/
  );
});

test('WebView receipt binds the host, engine package, CDP socket, and wv user agent', async () => {
  const { value } = await receipt({ runtime: 'webview' });
  assert.equal(value.chrome, null);
  assert.equal(value.state.chrome_top_resumed, false);
  assert.equal(value.state.runtime_host_top_resumed, true);
  assert.equal(value.browser_runtime.host.package, 'com.gsplat.competitive.playcanvas');
  assert.equal(value.browser_runtime.engine.package, 'com.google.android.webview');
  assert.equal(value.browser_runtime.engine.version_name, '150.0.7871.46');
  assert.equal(
    value.adb.forward.remote,
    'localabstract:webview_devtools_remote_17711'
  );
  const matched = verifyAndroidBrowserIdentity(value, {
    browserVersion: 'Chrome/150.0.7871.46',
    userAgent: 'Mozilla/5.0 (Linux; Android 15; A065; wv) Chrome/150.0.7871.46',
    platform: 'Linux armv8l'
  });
  assert.equal(matched.android_runtime_kind, 'webview');
  assert.equal(matched.adb_runtime_engine_package, 'com.google.android.webview');
});

test('optional expected receipt and pre/post identity are verified by stable fields', async () => {
  const { value } = await receipt();
  const bytes = Buffer.from(JSON.stringify(value));
  const envelope = await loadExpectedAndroidDeviceReceipt('/tmp/device.json', {
    readFileImpl: async () => bytes
  });
  assert.equal(verifyExpectedAndroidDeviceReceipt(envelope, value).identity_match, true);
  assert.equal(verifyStableAndroidDeviceIdentity(value, {
    ...value,
    phase: 'post',
    observed_at_utc: '2026-07-23T00:01:00.000Z'
  }).stable, true);
});

function png(width, height) {
  const bytes = Buffer.alloc(24);
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]).copy(bytes, 0);
  bytes.writeUInt32BE(13, 8);
  bytes.write('IHDR', 12, 'ascii');
  bytes.writeUInt32BE(width, 16);
  bytes.writeUInt32BE(height, 20);
  return bytes;
}

test('read-only screencap receipt proves physical PNG dimensions', async () => {
  const bytes = png(2412, 1080);
  assert.deepEqual(parsePngDimensions(bytes), { width: 2412, height: 1080 });
  const result = await captureAndroidScreenReceipt({
    execFile: async (_program, args) => {
      assert.deepEqual(args, ['-s', serial, 'exec-out', 'screencap', '-p']);
      return { stdout: bytes };
    },
    adbPath: 'adb',
    serial,
    expectedWidth: 2412,
    expectedHeight: 1080,
    now: () => '2026-07-23T00:00:00.000Z'
  });
  assert.equal(result.receipt.source, 'adb_exec_out_screencap_png_ihdr');
  assert.equal(result.receipt.command_effect, 'read_only');
  assert.deepEqual(result.png, bytes);
});
