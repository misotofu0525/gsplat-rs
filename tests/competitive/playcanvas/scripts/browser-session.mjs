import { androidRemoteConfig } from './android-device-receipt.mjs';

export const LOCAL_BROWSER_ARGS = Object.freeze([
  '--enable-unsafe-webgpu',
  '--enable-gpu',
  '--ignore-gpu-blocklist'
]);

function requiredRemoteText(environment, name) {
  const value = environment[name]?.trim();
  if (!value) throw new Error(`${name} is required for remote CDP`);
  return value;
}

function remotePort(environment) {
  const value = Number(environment.PLAYCANVAS_HARNESS_PORT ?? 4174);
  if (!Number.isSafeInteger(value) || value < 1 || value > 65535) {
    throw new Error('PLAYCANVAS_HARNESS_PORT must be an integer from 1 to 65535');
  }
  return value;
}

function remoteRefreshHz(environment) {
  const value = Number(requiredRemoteText(environment, 'PLAYCANVAS_REFRESH_HZ'));
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error('PLAYCANVAS_REFRESH_HZ must be a positive finite number');
  }
  return value;
}

export function browserSessionConfig(environment = process.env) {
  const endpoint = environment.PLAYCANVAS_CDP_ENDPOINT?.trim();
  if (!endpoint) {
    return {
      mode: 'local-launch',
      serverPort: 0,
      refreshHz: 60,
      refreshHzSource: 'configured',
      deviceLabel: null,
      deviceOs: null,
      endpoint: null
    };
  }

  let parsed;
  try {
    parsed = new URL(endpoint);
  } catch {
    throw new Error('PLAYCANVAS_CDP_ENDPOINT must be an absolute HTTP(S) URL');
  }
  if (!['http:', 'https:'].includes(parsed.protocol)) {
    throw new Error('PLAYCANVAS_CDP_ENDPOINT must be an absolute HTTP(S) URL');
  }
  if (parsed.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(parsed.hostname) ||
      !parsed.port || parsed.username || parsed.password || parsed.pathname !== '/' ||
      parsed.search || parsed.hash) {
    throw new Error(
      'remote Android PLAYCANVAS_CDP_ENDPOINT must be an ADB-forwarded ' +
      'http://127.0.0.1:<port> or http://localhost:<port> root URL'
    );
  }
  if (environment.PLAYCANVAS_DEVICE_LABEL?.trim() || environment.PLAYCANVAS_DEVICE_OS?.trim()) {
    throw new Error(
      'remote device identity is observed through ADB; ' +
      'PLAYCANVAS_DEVICE_LABEL and PLAYCANVAS_DEVICE_OS must not be supplied'
    );
  }
  const android = androidRemoteConfig(environment);

  return {
    mode: 'remote-cdp',
    serverPort: remotePort(environment),
    refreshHz: remoteRefreshHz(environment),
    refreshHzSource: 'externally_observed_required_config',
    deviceLabel: null,
    deviceOs: null,
    endpoint: parsed.toString().replace(/\/$/, ''),
    cdpPort: Number(parsed.port),
    ...android
  };
}

export function classifyBrowserFailure(error) {
  const text = `${error?.message ?? error ?? ''}\n${error?.stack ?? ''}`.toLowerCase();
  if (/\b(out of memory|oom)\b|allocation failed|failed to allocate/.test(text)) {
    return 'out_of_memory';
  }
  if (/navigator\.gpu|webgpu[^\n]*(unavailable|unsupported|not supported)|gpuadapter[^\n]*(missing|null)|onsubmittedworkdone is unavailable/.test(text)) {
    return 'webgpu_unavailable';
  }
  if (/target (closed|crashed)|session closed|browser has disconnected|connection closed|protocol error[^\n]*closed/.test(text)) {
    return 'browser_or_tab_crash';
  }
  if (/full-resolution receipt mismatch|qualification resolution mismatch/.test(text)) {
    return 'resolution_mismatch';
  }
  if (/device is locked|device is not awake|display is not on|not the top-resumed activity|browser page is not a focused, visible/.test(text)) {
    return 'device_not_foreground_unlocked';
  }
  if (/adb serial mismatch|adb forward receipt|device receipt identity mismatch|remote cdp target is not an observed android|does not match adb package/.test(text)) {
    return 'android_identity_mismatch';
  }
  if (/timeout|timed out/.test(text)) return 'timeout';
  return 'harness_failure';
}

export function isValidDevicePixelRatio(value) {
  return Number.isFinite(value) && value > 0;
}

export async function openBrowserSession({
  puppeteer,
  config,
  executablePath,
  headless,
  viewport
}) {
  let browser;
  let page;
  try {
    if (config.mode === 'remote-cdp') {
      browser = await puppeteer.connect({
        browserURL: config.endpoint,
        defaultViewport: null
      });
    } else {
      browser = await puppeteer.launch({
        executablePath,
        headless,
        defaultViewport: viewport,
        args: [...LOCAL_BROWSER_ARGS]
      });
    }
    if (config.mode === 'remote-cdp') {
      if (config.targetMode === 'existing-page') {
        const pages = await browser.pages();
        if (pages.length !== 1) {
          throw new Error(`Android WebView CDP must expose exactly one page, observed ${pages.length}`);
        }
        [page] = pages;
      } else {
        page = await browser.newPage();
      }
      await page.bringToFront();
    } else {
      page = await browser.newPage();
    }
  } catch (error) {
    if (config.mode === 'remote-cdp') {
      try {
        if (config.targetMode !== 'existing-page') await page?.close();
      } finally {
        browser?.disconnect();
      }
    } else {
      await browser?.close();
    }
    throw error;
  }

  return {
    browser,
    page,
    async close() {
      if (config.mode === 'remote-cdp') {
        try {
          if (config.targetMode !== 'existing-page') {
            await page.close();
          }
        } finally {
          browser.disconnect();
        }
      } else {
        await browser.close();
      }
    }
  };
}
