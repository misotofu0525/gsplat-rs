import { link, mkdir, unlink, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import process from 'node:process';

export const BROWSER_OWNERSHIP_SCHEMA = 'gsplat-q1-browser-process-ownership/v1';

export function browserOwnershipConfig(environment = process.env, required = false) {
  const marker = environment.GSPLAT_Q1_BROWSER_OWNER_MARKER?.trim() ?? '';
  const userDataDir = environment.GSPLAT_Q1_BROWSER_USER_DATA_DIR?.trim() ?? '';
  const handshakePath = environment.GSPLAT_Q1_BROWSER_HANDSHAKE_PATH?.trim() ?? '';
  const present = [marker, userDataDir, handshakePath].filter(Boolean).length;
  if (present === 0 && !required) return null;
  if (present !== 3 || !/^[a-z0-9-]{24,96}$/.test(marker)) {
    throw new Error('formal browser ownership requires one valid marker, user-data-dir, and handshake path');
  }
  if (!userDataDir.startsWith('/') || !handshakePath.startsWith('/')) {
    throw new Error('formal browser ownership paths must be absolute');
  }
  if (/\s/.test(userDataDir)) {
    throw new Error('formal browser user-data-dir cannot contain whitespace');
  }
  return {
    marker,
    markerArg: `--user-data-dir=${resolve(userDataDir)}`,
    userDataDir: resolve(userDataDir),
    handshakePath: resolve(handshakePath),
  };
}

export async function publishBrowserOwnershipHandshake(browser, config) {
  if (!config) return null;
  const browserProcess = browser.process?.();
  if (!Number.isSafeInteger(browserProcess?.pid) || browserProcess.pid <= 0) {
    throw new Error('formal browser launch did not expose a positive browser PID');
  }
  const spawnargs = browserProcess.spawnargs ?? [];
  if (!spawnargs.includes(config.markerArg)) {
    throw new Error('formal browser process omitted its exact user-data-dir ownership argument');
  }
  await mkdir(dirname(config.handshakePath), { recursive: true });
  const temporary = `${config.handshakePath}.tmp-${process.pid}`;
  const receipt = {
    schema: BROWSER_OWNERSHIP_SCHEMA,
    marker: config.marker,
    marker_arg: config.markerArg,
    user_data_dir: config.userDataDir,
    producer_pid: process.pid,
    producer_ppid: process.ppid,
    browser_pid: browserProcess.pid,
    browser_spawnfile: browserProcess.spawnfile ?? null,
    browser_spawnargs: spawnargs,
  };
  await writeFile(temporary, `${JSON.stringify(receipt, null, 2)}\n`, { flag: 'wx' });
  try {
    await link(temporary, config.handshakePath);
  } finally {
    await unlink(temporary);
  }
  return receipt;
}
