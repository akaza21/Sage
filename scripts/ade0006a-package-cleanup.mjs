import { readFile, realpath, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { sha256, SHA256 } from "./ade0006a-c0b-common.mjs";
import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import {
  optionalStat,
  packageRoots,
  readLedger,
  reconcile,
  scanTree,
  withAdmissionLock,
  writeLedger,
} from "./ade0006a-package-retention-ledger.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const idPattern = /^[a-z0-9][a-z0-9-]{0,63}$/u;

async function finishRecordedCleanup(sourceRoot, packageRoot, cleanup, frozen) {
  const { curatedRoot } = packageRoots(sourceRoot);
  for (const removed of cleanup.removedCampaigns) {
    const campaignRoot = path.join(packageRoot, removed.campaignId);
    const campaignStat = await optionalStat(campaignRoot);
    const observed = await scanTree(
      campaignRoot,
      frozen.evidence.campaignBounds,
    );
    if (
      observed.files !== 0 &&
      (observed.files !== removed.rawFiles ||
        observed.bytes !== removed.rawBytes)
    ) {
      throw new Error("cleanup-reconciliation-failed");
    }
    if (campaignStat) {
      if (!campaignStat.isDirectory()) {
        throw new Error("cleanup-reconciliation-failed");
      }
      await rm(campaignRoot, { recursive: true });
    }
    if (removed.curatedReceipt) {
      const receiptPath = path.join(curatedRoot, `${removed.campaignId}.json`);
      const receiptStat = await optionalStat(receiptPath);
      if (receiptStat) {
        const bytes = await readFile(receiptPath);
        if (
          !receiptStat.isFile() ||
          bytes.byteLength !== removed.curatedReceipt.bytes ||
          sha256(bytes) !== removed.curatedReceipt.sha256
        ) {
          throw new Error("cleanup-reconciliation-failed");
        }
        await rm(receiptPath);
      }
    }
  }
}

export async function cleanupPackageCandidate({
  sourceRoot,
  cleanupId,
  allocationIdentity,
  completedAtUtc,
  allowCurated = false,
  frozen,
}) {
  if (
    !idPattern.test(cleanupId ?? "") ||
    !SHA256.test(allocationIdentity ?? "") ||
    Number.isNaN(Date.parse(completedAtUtc ?? ""))
  ) {
    throw new Error("cleanup-request-invalid");
  }
  return withAdmissionLock(sourceRoot, frozen, async (packageRoot) => {
    const ledger = await readLedger(packageRoot, frozen);
    for (const cleanup of ledger.cleanups) {
      await finishRecordedCleanup(sourceRoot, packageRoot, cleanup, frozen);
    }
    const existing = ledger.cleanups.find(
      (cleanup) => cleanup.cleanupId === cleanupId,
    );
    if (existing) {
      if (existing.allocationIdentity !== allocationIdentity) {
        throw new Error("cleanup-id-conflict");
      }
      return existing;
    }
    const actual = await reconcile(sourceRoot, packageRoot, ledger, frozen);
    const selected = ledger.entries
      .filter((entry) => entry.allocationIdentity === allocationIdentity)
      .sort((left, right) =>
        left.completedAtUtc.localeCompare(right.completedAtUtc),
      );
    if (
      selected.length < 1 ||
      selected.some((entry) => entry.state === "active") ||
      (!allowCurated && selected.some((entry) => entry.curatedReceipt))
    ) {
      throw new Error("cleanup-candidate-ineligible");
    }
    const selectedIds = new Set(selected.map(({ campaignId }) => campaignId));
    const removingFailure = selected.some(
      ({ state }) => state === "failed-unsummarized",
    );
    const remainingFailures = ledger.entries.filter(
      (entry) =>
        entry.state === "failed-unsummarized" &&
        !selectedIds.has(entry.campaignId),
    ).length;
    if (removingFailure && remainingFailures < 1) {
      throw new Error("cleanup-sole-unsummarized-failure");
    }
    const removedCampaigns = selected.map((entry) => {
      const observed = actual.get(entry.campaignId);
      return {
        campaignId: entry.campaignId,
        rawFiles: observed.files,
        rawBytes: observed.bytes,
        curatedReceipt: entry.curatedReceipt,
      };
    });
    const cleanup = {
      cleanupId,
      allocationIdentity,
      completedAtUtc,
      removedCampaigns,
      removedFiles: removedCampaigns.reduce(
        (total, removed) =>
          total + removed.rawFiles + (removed.curatedReceipt ? 1 : 0),
        0,
      ),
      removedBytes: removedCampaigns.reduce(
        (total, removed) =>
          total + removed.rawBytes + (removed.curatedReceipt?.bytes ?? 0),
        0,
      ),
    };
    ledger.entries = ledger.entries.filter(
      (entry) => !selectedIds.has(entry.campaignId),
    );
    ledger.cleanups.push(cleanup);
    await writeLedger(packageRoot, ledger, frozen);
    await finishRecordedCleanup(sourceRoot, packageRoot, cleanup, frozen);
    return cleanup;
  });
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index < 0 ? undefined : process.argv[index + 1];
}

if (process.argv[1] && path.resolve(process.argv[1]) === scriptPath) {
  try {
    const sourceRoot = await realpath(argument("--source-root"));
    const cleanup = await cleanupPackageCandidate({
      sourceRoot,
      cleanupId: argument("--cleanup-id"),
      allocationIdentity: argument("--allocation-identity"),
      completedAtUtc: new Date().toISOString(),
      allowCurated: process.argv.includes("--allow-curated"),
      frozen: loadFrozenContracts(sourceRoot),
    });
    process.stdout.write(JSON.stringify(cleanup));
  } catch (error) {
    process.stderr.write(`package-cleanup: ${error.message}\n`);
    process.exitCode = 1;
  }
}
