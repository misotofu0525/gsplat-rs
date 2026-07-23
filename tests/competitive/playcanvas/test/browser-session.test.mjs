import assert from 'node:assert/strict';
import test from 'node:test';
import {
  browserSessionConfig,
  classifyBrowserFailure,
  isValidDevicePixelRatio,
  openBrowserSession
} from '../scripts/browser-session.mjs';

const viewport = { width: 2412, height: 1080, deviceScaleFactor: 1 };

test('local browser session preserves launch and close behavior', async () => {
  const calls = [];
  const page = { setViewport: async () => calls.push('unexpected-set-viewport') };
  const browser = {
    newPage: async () => {
      calls.push('new-page');
      return page;
    },
    close: async () => calls.push('browser-close')
  };
  const puppeteer = {
    launch: async (options) => {
      calls.push(['launch', options]);
      return browser;
    },
    connect: async () => assert.fail('local session must not connect')
  };
  const config = browserSessionConfig({});
  const session = await openBrowserSession({
    puppeteer,
    config,
    executablePath: '/Applications/Google Chrome',
    headless: true,
    viewport
  });
  await session.close();

  assert.equal(config.mode, 'local-launch');
  assert.equal(config.serverPort, 0);
  assert.deepEqual(calls, [
    ['launch', {
      executablePath: '/Applications/Google Chrome',
      headless: true,
      defaultViewport: viewport,
      args: ['--enable-unsafe-webgpu', '--enable-gpu', '--ignore-gpu-blocklist']
    }],
    'new-page',
    'browser-close'
  ]);
});

test('remote browser session preserves native metrics and only disconnects', async () => {
  const calls = [];
  const page = {
    bringToFront: async () => calls.push('bring-to-front'),
    close: async () => calls.push('page-close')
  };
  const browser = {
    newPage: async () => {
      calls.push('new-page');
      return page;
    },
    disconnect: () => calls.push('browser-disconnect'),
    close: async () => assert.fail('remote session must not close the device browser')
  };
  const puppeteer = {
    launch: async () => assert.fail('remote session must not launch'),
    connect: async (options) => {
      calls.push(['connect', options]);
      return browser;
    }
  };
  const config = browserSessionConfig({
    PLAYCANVAS_CDP_ENDPOINT: 'http://127.0.0.1:9222/',
    PLAYCANVAS_HARNESS_PORT: '4174',
    PLAYCANVAS_REFRESH_HZ: '120',
    PLAYCANVAS_ADB_SERIAL: '033ed212'
  });
  const session = await openBrowserSession({
    puppeteer,
    config,
    executablePath: null,
    headless: false,
    viewport
  });
  await session.close();

  assert.deepEqual(config, {
    mode: 'remote-cdp',
    serverPort: 4174,
    refreshHz: 120,
    refreshHzSource: 'externally_observed_required_config',
    deviceLabel: null,
    deviceOs: null,
    endpoint: 'http://127.0.0.1:9222',
    cdpPort: 9222,
    adbPath: 'adb',
    adbSerial: '033ed212',
    androidRuntimeKind: 'chrome',
    hostPackage: 'com.android.chrome',
    runtimePackage: 'com.android.chrome',
    cdpSocket: 'chrome_devtools_remote',
    targetMode: 'new-page',
    expectedReceiptPath: null
  });
  assert.deepEqual(calls, [
    ['connect', { browserURL: 'http://127.0.0.1:9222', defaultViewport: null }],
    'new-page',
    'bring-to-front',
    'page-close',
    'browser-disconnect'
  ]);
});

test('remote WebView session reuses one page without emulating device metrics', async () => {
  const calls = [];
  const page = {
    bringToFront: async () => calls.push('bring-to-front'),
    close: async () => assert.fail('WebView target must not be closed')
  };
  const browser = {
    pages: async () => {
      calls.push('pages');
      return [page];
    },
    newPage: async () => assert.fail('WebView must reuse its single page'),
    disconnect: () => calls.push('browser-disconnect')
  };
  const puppeteer = {
    connect: async (options) => {
      calls.push(['connect', options]);
      return browser;
    }
  };
  const config = browserSessionConfig({
    PLAYCANVAS_CDP_ENDPOINT: 'http://127.0.0.1:9223',
    PLAYCANVAS_REFRESH_HZ: '90',
    PLAYCANVAS_ADB_SERIAL: '033ed212',
    PLAYCANVAS_ANDROID_RUNTIME: 'webview',
    PLAYCANVAS_ANDROID_HOST_PACKAGE: 'com.gsplat.competitive.playcanvas',
    PLAYCANVAS_ANDROID_CDP_SOCKET: 'webview_devtools_remote_17711'
  });
  const session = await openBrowserSession({
    puppeteer,
    config,
    executablePath: null,
    headless: false,
    viewport
  });
  await session.close();

  assert.equal(config.targetMode, 'existing-page');
  assert.deepEqual(calls, [
    ['connect', { browserURL: 'http://127.0.0.1:9223', defaultViewport: null }],
    'pages',
    'bring-to-front',
    'browser-disconnect'
  ]);
});

test('remote browser session fails closed without device receipts', () => {
  assert.throws(
    () => browserSessionConfig({ PLAYCANVAS_CDP_ENDPOINT: 'http://127.0.0.1:9222' }),
    /PLAYCANVAS_ADB_SERIAL/
  );
  assert.throws(
    () => browserSessionConfig({
      PLAYCANVAS_CDP_ENDPOINT: 'http://127.0.0.1:9222',
      PLAYCANVAS_ADB_SERIAL: '033ed212'
    }),
    /PLAYCANVAS_REFRESH_HZ is required/
  );
  assert.throws(
    () => browserSessionConfig({
      PLAYCANVAS_CDP_ENDPOINT: 'file:///tmp/devtools',
      PLAYCANVAS_REFRESH_HZ: '120',
      PLAYCANVAS_ADB_SERIAL: '033ed212'
    }),
    /absolute HTTP\(S\) URL/
  );
});

test('remote browser session disconnects even when its page refuses to close', async () => {
  const calls = [];
  const closeError = new Error('target already crashed');
  const page = {
    setViewport: async () => {},
    bringToFront: async () => {},
    close: async () => {
      calls.push('page-close');
      throw closeError;
    }
  };
  const browser = {
    newPage: async () => page,
    disconnect: () => calls.push('browser-disconnect')
  };
  const puppeteer = {
    connect: async () => browser
  };
  const config = browserSessionConfig({
    PLAYCANVAS_CDP_ENDPOINT: 'http://127.0.0.1:9222',
    PLAYCANVAS_REFRESH_HZ: '120',
    PLAYCANVAS_ADB_SERIAL: '033ed212'
  });
  const session = await openBrowserSession({
    puppeteer,
    config,
    executablePath: null,
    headless: false,
    viewport
  });

  await assert.rejects(session.close(), closeError);
  assert.deepEqual(calls, ['page-close', 'browser-disconnect']);
});

test('remote failure classification keeps unavailable, crash, and OOM blockers distinct', () => {
  assert.equal(
    classifyBrowserFailure(new Error('navigator.gpu is unavailable')),
    'webgpu_unavailable'
  );
  assert.equal(
    classifyBrowserFailure(new Error('Protocol error (Runtime.callFunctionOn): Target closed')),
    'browser_or_tab_crash'
  );
  assert.equal(
    classifyBrowserFailure(new Error('GPU buffer allocation failed: out of memory')),
    'out_of_memory'
  );
  assert.equal(
    classifyBrowserFailure(new Error('remote full-resolution receipt mismatch')),
    'resolution_mismatch'
  );
});

test('remote identity cannot be supplied through legacy label or OS strings', () => {
  assert.throws(
    () => browserSessionConfig({
      PLAYCANVAS_CDP_ENDPOINT: 'http://127.0.0.1:9222',
      PLAYCANVAS_REFRESH_HZ: '120',
      PLAYCANVAS_ADB_SERIAL: '033ed212',
      PLAYCANVAS_DEVICE_LABEL: 'Nothing A065'
    }),
    /identity is observed through ADB/
  );
  assert.throws(
    () => browserSessionConfig({
      PLAYCANVAS_CDP_ENDPOINT: 'http://desktop.example:9222',
      PLAYCANVAS_REFRESH_HZ: '120',
      PLAYCANVAS_ADB_SERIAL: '033ed212'
    }),
    /ADB-forwarded/
  );
});

test('remote DPR gate accepts native positive density and rejects invalid values', () => {
  assert.equal(isValidDevicePixelRatio(1), true);
  assert.equal(isValidDevicePixelRatio(2.625), true);
  assert.equal(isValidDevicePixelRatio(0), false);
  assert.equal(isValidDevicePixelRatio(Number.NaN), false);
});
