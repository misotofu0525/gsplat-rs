import { access, mkdir, writeFile } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';
import puppeteer from 'puppeteer-core';
import { startServer } from './server.mjs';

const harnessRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(harnessRoot, '..', '..', '..');
const outputRoot = resolve(repoRoot, 'target/benchmarks/playcanvas-path-smoke');
const chromeCandidates = [
  process.env.CHROME_PATH,
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium'
].filter(Boolean);

async function findChrome() {
  for (const candidate of chromeCandidates) {
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {}
  }
  return null;
}

await mkdir(outputRoot, { recursive: true });
const chrome = await findChrome();
if (!chrome) {
  const blocker = { status: 'blocked', reason: 'no supported Chrome/Chromium executable found', candidates: chromeCandidates };
  await writeFile(resolve(outputRoot, 'blocker.json'), `${JSON.stringify(blocker, null, 2)}\n`);
  console.error(JSON.stringify(blocker));
  process.exit(2);
}

const { server, port } = await startServer(0);
let browser;
try {
  browser = await puppeteer.launch({
    executablePath: chrome,
    headless: process.env.HEADLESS !== '0',
    defaultViewport: { width: 640, height: 480, deviceScaleFactor: 1 },
    args: ['--enable-unsafe-webgpu', '--enable-gpu', '--ignore-gpu-blocklist']
  });
  const page = await browser.newPage();
  const browserLog = [];
  page.on('console', (message) => browserLog.push(`${message.type()}: ${message.text()}`));
  page.on('pageerror', (error) => browserLog.push(`pageerror: ${error.stack ?? error.message}`));
  await page.goto(`http://127.0.0.1:${port}/`, { waitUntil: 'networkidle0', timeout: 30_000 });
  await page.waitForFunction(
    () => window.__PLAYCANVAS_HARNESS_RESULT__ || window.__PLAYCANVAS_HARNESS_ERROR__,
    { timeout: 30_000 }
  );
  const outcome = await page.evaluate(() => window.__PLAYCANVAS_HARNESS_RESULT__ ?? window.__PLAYCANVAS_HARNESS_ERROR__);
  await writeFile(resolve(outputRoot, 'runtime.log'), `${browserLog.join('\n')}\n`);
  await writeFile(resolve(outputRoot, 'result.json'), `${JSON.stringify(outcome, null, 2)}\n`);
  if (outcome.status !== 'ready_for_pre_timing_capture') {
    console.error(JSON.stringify(outcome));
    process.exitCode = 2;
  } else {
    const screenshot = resolve(outputRoot, 'pre-timing.png');
    await page.screenshot({ path: screenshot });
    console.log(JSON.stringify({ ...outcome, screenshot }));
  }
} catch (error) {
  const blocker = { status: 'blocked', reason: error.message, stack: error.stack, chrome };
  await writeFile(resolve(outputRoot, 'blocker.json'), `${JSON.stringify(blocker, null, 2)}\n`);
  console.error(JSON.stringify(blocker));
  process.exitCode = 2;
} finally {
  await browser?.close();
  await new Promise((resolvePromise) => server.close(resolvePromise));
}
