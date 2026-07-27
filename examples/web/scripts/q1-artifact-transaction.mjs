import { createHash } from "node:crypto";
import { access, mkdir, rename, stat, writeFile } from "node:fs/promises";
import { basename, dirname, relative, resolve, sep } from "node:path";

const BLOCKER_SCHEMA = "gsplat-q1-collector-blocker/v1";
const CLEANUP_BLOCKER_SCHEMA = "gsplat-q1-cleanup-blocker/v1";

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

function errorText(error) {
  return String(error?.stack ?? error?.message ?? error);
}

function cellKey(seriesRoot, finalDirectory) {
  const cell = relative(seriesRoot, finalDirectory);
  if (cell === "" || cell === ".." || cell.startsWith(`..${sep}`)) {
    throw new TypeError("Q1 artifact destination escaped its series root");
  }
  return {
    cell: cell.split(sep).join("/"),
    digest: createHash("sha256").update(cell).digest("hex").slice(0, 16),
  };
}

export class Q1ArtifactTransaction {
  static async claim({ seriesRoot, finalDirectory, collectionSessionId }) {
    const root = resolve(seriesRoot);
    const final = resolve(finalDirectory);
    const rootState = await stat(root);
    if (!rootState.isDirectory()) throw new TypeError("Q1 series root is not a directory");
    const { cell, digest } = cellKey(root, final);
    if (resolve(dirname(final)) === final || !(await stat(dirname(final))).isDirectory()) {
      throw new TypeError("Q1 artifact parent must already exist");
    }
    const label = basename(final);
    const claim = resolve(root, `.q1-${label}-${digest}.claim.json`);
    const blocker = resolve(root, `.q1-${label}-${digest}.blocker.json`);
    const cleanupBlocker = resolve(root, `.q1-${label}-${digest}.cleanup-blocker.json`);
    const staging = resolve(dirname(final), `.${label}.staging-${digest}`);
    if (await exists(final) || await exists(blocker) || await exists(cleanupBlocker)
        || await exists(staging)) {
      throw new Error(`Q1 artifact cell is not fresh: ${cell}`);
    }
    const claimReceipt = {
      schema: "gsplat-q1-artifact-claim/v1",
      cell,
      collection_session_id: collectionSessionId,
      automatic_retry: false,
    };
    await writeFile(claim, `${JSON.stringify(claimReceipt, null, 2)}\n`, { flag: "wx" });
    try {
      if (await exists(final)) throw new Error(`Q1 destination appeared during claim: ${cell}`);
      await mkdir(staging);
    } catch (error) {
      await writeFile(blocker, `${JSON.stringify({
        schema: BLOCKER_SCHEMA,
        cell,
        phase: "claim",
        reason: errorText(error),
        automatic_retry: false,
      }, null, 2)}\n`, { flag: "wx" });
      throw error;
    }
    return new Q1ArtifactTransaction({
      root, final, staging, claim, blocker, cleanupBlocker, cell,
    });
  }

  constructor({ root, final, staging, claim, blocker, cleanupBlocker, cell }) {
    this.seriesRoot = root;
    this.finalDirectory = final;
    this.stagingDirectory = staging;
    this.claimPath = claim;
    this.blockerPath = blocker;
    this.cleanupBlockerPath = cleanupBlocker;
    this.cell = cell;
    this.cleanupComplete = false;
    this.failed = false;
    this.published = false;
  }

  async recordFailure(error, phase) {
    this.failed = true;
    if (!(await exists(this.blockerPath))) {
      await writeFile(this.blockerPath, `${JSON.stringify({
        schema: BLOCKER_SCHEMA,
        cell: this.cell,
        phase,
        reason: errorText(error),
        automatic_retry: false,
        final_published: false,
      }, null, 2)}\n`, { flag: "wx" });
    }
  }

  async recordCleanupFailure(error) {
    this.failed = true;
    await writeFile(this.cleanupBlockerPath, `${JSON.stringify({
      schema: CLEANUP_BLOCKER_SCHEMA,
      cell: this.cell,
      reason: errorText(error),
      automatic_retry: false,
      final_published: false,
    }, null, 2)}\n`, { flag: "wx" });
    await this.recordFailure(error, "cleanup");
  }

  markCleanupComplete() {
    if (this.failed || this.published) {
      throw new Error("Q1 cleanup cannot complete after failure or publication");
    }
    this.cleanupComplete = true;
  }

  async publish() {
    if (!this.cleanupComplete || this.failed || this.published
        || await exists(this.blockerPath) || await exists(this.cleanupBlockerPath)) {
      throw new Error("Q1 artifact cannot publish before stable cleanup or after failure");
    }
    if (await exists(this.finalDirectory)) {
      throw new Error(`Q1 destination appeared before publish: ${this.cell}`);
    }
    await rename(this.stagingDirectory, this.finalDirectory);
    this.published = true;
    return this.finalDirectory;
  }
}
