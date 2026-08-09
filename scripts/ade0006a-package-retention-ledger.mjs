import {
  lstat,
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

import { withAdeCampaignAdmission } from "./ade0006a-admission.mjs";
import {
  exactKeys,
  inspectSanitized,
  sha256,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import { validatePackageCleanup } from "./ade0006a-package-cleanup-validation.mjs";

const ledgerName = "retention-ledger-v1.json";
const nextLedgerName = "retention-ledger-v1.next.json";
const campaignIdPattern = /^[a-z0-9][a-z0-9-]{0,63}$/;
const states = new Set([
  "active",
  "completed-passed",
  "completed-failed",
  "failed-unsummarized",
]);

export function packageRoots(sourceRoot) {
  const evidenceRoot = path.join(sourceRoot, "local-evidence");
  const adeRoot = path.join(evidenceRoot, "ade0006a");
  const packageRoot = path.join(evidenceRoot, "ade0006a-package");
  const curatedRoot = path.join(
    sourceRoot,
    "docs",
    "work-items",
    "ADE-0006A-acp-session-foundation",
    "evidence",
    "package",
  );
  return { evidenceRoot, adeRoot, packageRoot, curatedRoot };
}

export async function optionalStat(filePath) {
  try {
    return await lstat(filePath);
  } catch (error) {
    if (error.code === "ENOENT") return undefined;
    throw error;
  }
}

export async function scanTree(root, bounds, depth = 0) {
  const item = await optionalStat(root);
  if (!item) return { files: 0, bytes: 0 };
  if (!item.isDirectory() || depth > 8) {
    throw new Error("ledger-reconciliation-failed");
  }
  let files = 0;
  let bytes = 0;
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const child = path.join(root, entry.name);
    if (entry.isDirectory()) {
      const nested = await scanTree(child, bounds, depth + 1);
      files += nested.files;
      bytes += nested.bytes;
    } else if (entry.isFile()) {
      const childStat = await lstat(child);
      files += 1;
      bytes += childStat.size;
    } else {
      throw new Error("ledger-reconciliation-failed");
    }
    if (files > bounds.globalFiles || bytes > bounds.globalBytes) {
      throw new Error("capacity-rejected");
    }
  }
  return { files, bytes };
}

function emptyLedger() {
  return {
    schemaVersion: 1,
    workItem: "ADE-0006A",
    phase: "Package",
    entries: [],
    cleanups: [],
  };
}

function validateEntry(entry, errors, index, frozen) {
  const label = `retentionLedger.entries[${index}]`;
  exactKeys(
    errors,
    entry,
    [
      "campaignId",
      "allocationIdentity",
      "sourceAggregateSha256",
      "contractManifestSha256",
      "packageContractSha256",
      "state",
      "startedAtUtc",
      "completedAtUtc",
      "reservedRawFiles",
      "reservedRawBytes",
      "actualRawFiles",
      "actualRawBytes",
      "curatedReceipt",
      "failureCode",
    ],
    label,
  );
  const reservation = frozen.evidence.packageCampaignRetention.reservation;
  const terminal = entry?.state !== "active";
  if (
    !campaignIdPattern.test(entry?.campaignId ?? "") ||
    !SHA256.test(entry?.allocationIdentity ?? "") ||
    !SHA256.test(entry?.sourceAggregateSha256 ?? "") ||
    !SHA256.test(entry?.contractManifestSha256 ?? "") ||
    !SHA256.test(entry?.packageContractSha256 ?? "") ||
    !states.has(entry?.state) ||
    Number.isNaN(Date.parse(entry?.startedAtUtc ?? "")) ||
    entry?.reservedRawFiles !== reservation.rawFiles ||
    entry?.reservedRawBytes !== reservation.rawBytes ||
    (terminal && Number.isNaN(Date.parse(entry?.completedAtUtc ?? ""))) ||
    (!terminal && entry?.completedAtUtc !== null)
  ) {
    errors.push(`${label} identity or lifecycle is invalid`);
  }
  if (terminal) {
    if (
      !Number.isSafeInteger(entry?.actualRawFiles) ||
      entry.actualRawFiles < 0 ||
      entry.actualRawFiles > entry.reservedRawFiles ||
      !Number.isSafeInteger(entry?.actualRawBytes) ||
      entry.actualRawBytes < 0 ||
      entry.actualRawBytes > entry.reservedRawBytes
    ) {
      errors.push(`${label} actual retention is invalid`);
    }
  } else if (entry?.actualRawFiles !== null || entry?.actualRawBytes !== null) {
    errors.push(`${label} active retention must remain unresolved`);
  }
  const completed = entry?.state?.startsWith("completed-");
  if (completed) {
    exactKeys(
      errors,
      entry?.curatedReceipt,
      ["relativePath", "sha256", "bytes"],
      `${label}.curatedReceipt`,
    );
    if (
      entry.curatedReceipt?.relativePath !==
        `docs/work-items/ADE-0006A-acp-session-foundation/evidence/package/${entry.campaignId}.json` ||
      !SHA256.test(entry.curatedReceipt?.sha256 ?? "") ||
      !Number.isSafeInteger(entry.curatedReceipt?.bytes) ||
      entry.curatedReceipt.bytes < 1 ||
      entry.curatedReceipt.bytes > reservation.curatedBytes ||
      entry?.failureCode !== null
    ) {
      errors.push(`${label} completed receipt is invalid`);
    }
  } else if (
    entry?.curatedReceipt !== null ||
    (entry?.state === "failed-unsummarized" &&
      !frozen.package.retention.collectionFailureCodes.includes(
        entry?.failureCode,
      )) ||
    (entry?.state === "active" && entry?.failureCode !== null)
  ) {
    errors.push(`${label} failure disposition is invalid`);
  }
}

export async function readLedger(packageRoot, frozen) {
  const ledgerPath = path.join(packageRoot, ledgerName);
  if (!(await optionalStat(ledgerPath))) return emptyLedger();
  if (await optionalStat(path.join(packageRoot, nextLedgerName))) {
    throw new Error("ledger-reconciliation-failed");
  }
  const bytes = await readFile(ledgerPath);
  if (bytes.byteLength > frozen.evidence.packageCampaignRetention.ledgerBytes) {
    throw new Error("ledger-reconciliation-failed");
  }
  let ledger;
  try {
    ledger = JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new Error("ledger-reconciliation-failed");
  }
  if (
    ledger &&
    typeof ledger === "object" &&
    !Array.isArray(ledger) &&
    Object.keys(ledger).sort().join("\0") ===
      ["schemaVersion", "workItem", "phase", "entries"].sort().join("\0")
  ) {
    ledger.cleanups = [];
  }
  const errors = [];
  exactKeys(
    errors,
    ledger,
    ["schemaVersion", "workItem", "phase", "entries", "cleanups"],
    "retentionLedger",
  );
  if (
    ledger?.schemaVersion !== 1 ||
    ledger?.workItem !== "ADE-0006A" ||
    ledger?.phase !== "Package" ||
    !Array.isArray(ledger?.entries) ||
    !Array.isArray(ledger?.cleanups) ||
    ledger.entries.length >
      frozen.evidence.packageCampaignRetention.ledgerEntries ||
    ledger.cleanups.length >
      frozen.evidence.packageCampaignRetention.cleanupRecords
  ) {
    errors.push("retentionLedger header is invalid");
  } else {
    ledger.entries.forEach((entry, index) =>
      validateEntry(entry, errors, index, frozen),
    );
    ledger.cleanups.forEach((cleanup, index) =>
      validatePackageCleanup(cleanup, errors, index, frozen),
    );
    if (
      new Set(ledger.entries.map(({ campaignId }) => campaignId)).size !==
      ledger.entries.length
    ) {
      errors.push("retentionLedger campaign IDs are not unique");
    }
    const cleanupIds = ledger.cleanups.map(({ cleanupId }) => cleanupId);
    const removedIds = ledger.cleanups.flatMap(({ removedCampaigns }) =>
      removedCampaigns.map(({ campaignId }) => campaignId),
    );
    if (
      new Set(cleanupIds).size !== cleanupIds.length ||
      new Set(removedIds).size !== removedIds.length ||
      removedIds.some((campaignId) =>
        ledger.entries.some((entry) => entry.campaignId === campaignId),
      )
    ) {
      errors.push("retentionLedger cleanup identities are not unique");
    }
  }
  inspectSanitized(ledger, errors, "package-retention-ledger");
  if (errors.length > 0) throw new Error("ledger-reconciliation-failed");
  return ledger;
}

const ledgerFileOperations = { writeFile, rename, rm };

export async function writeLedger(
  packageRoot,
  ledger,
  frozen,
  operations = ledgerFileOperations,
) {
  const bytes = Buffer.from(JSON.stringify(ledger));
  if (bytes.byteLength > frozen.evidence.packageCampaignRetention.ledgerBytes) {
    throw new Error("capacity-rejected");
  }
  const nextPath = path.join(packageRoot, nextLedgerName);
  const ledgerPath = path.join(packageRoot, ledgerName);
  let staged = false;
  try {
    await operations.writeFile(nextPath, bytes, { flag: "wx" });
    staged = true;
    await operations.rename(nextPath, ledgerPath);
  } catch (error) {
    if (staged || error.code !== "EEXIST") {
      await operations.rm(nextPath, { force: true });
    }
    throw error;
  }
}

export async function withAdmissionLock(sourceRoot, frozen, action) {
  const { packageRoot } = packageRoots(sourceRoot);
  return withAdeCampaignAdmission({
    sourceRoot,
    frozen,
    phase: "Package",
    action: async () => {
      await mkdir(packageRoot, { recursive: true });
      return action(packageRoot);
    },
  });
}

export async function reconcile(
  sourceRoot,
  packageRoot,
  ledger,
  frozen,
  allowActiveReceiptCampaign,
) {
  const { curatedRoot } = packageRoots(sourceRoot);
  const allowed = new Set([ledgerName]);
  const entries = await readdir(packageRoot, { withFileTypes: true });
  const knownCampaigns = new Set(
    ledger.entries.map(({ campaignId }) => campaignId),
  );
  const cleanedCampaigns = new Set(
    ledger.cleanups.flatMap(({ removedCampaigns }) =>
      removedCampaigns.map(({ campaignId }) => campaignId),
    ),
  );
  for (const entry of entries) {
    if (allowed.has(entry.name)) continue;
    if (
      !entry.isDirectory() ||
      (!knownCampaigns.has(entry.name) && !cleanedCampaigns.has(entry.name))
    ) {
      throw new Error("ledger-reconciliation-failed");
    }
  }
  const actualByCampaign = new Map();
  for (const entry of ledger.entries) {
    const campaignRoot = path.join(packageRoot, entry.campaignId);
    if (!(await optionalStat(campaignRoot))) {
      throw new Error("ledger-reconciliation-failed");
    }
    const actual = await scanTree(campaignRoot, frozen.evidence.campaignBounds);
    actualByCampaign.set(entry.campaignId, actual);
    const receiptPath = path.join(curatedRoot, `${entry.campaignId}.json`);
    const receiptStat = await optionalStat(receiptPath);
    if (entry.state === "active") {
      if (receiptStat && entry.campaignId !== allowActiveReceiptCampaign) {
        throw new Error("ledger-reconciliation-failed");
      }
    } else if (entry.state === "failed-unsummarized") {
      if (
        receiptStat ||
        actual.files !== entry.actualRawFiles ||
        actual.bytes !== entry.actualRawBytes
      ) {
        throw new Error("ledger-reconciliation-failed");
      }
    } else {
      if (
        !receiptStat?.isFile() ||
        actual.files !== entry.actualRawFiles ||
        actual.bytes !== entry.actualRawBytes
      ) {
        throw new Error("ledger-reconciliation-failed");
      }
      const receiptBytes = await readFile(receiptPath);
      if (
        receiptBytes.byteLength !== entry.curatedReceipt.bytes ||
        sha256(receiptBytes) !== entry.curatedReceipt.sha256
      ) {
        throw new Error("ledger-reconciliation-failed");
      }
    }
  }
  return actualByCampaign;
}

export function projectedRaw(entry, actual) {
  return entry.state === "active"
    ? {
        files: Math.max(actual.files, entry.reservedRawFiles),
        bytes: Math.max(actual.bytes, entry.reservedRawBytes),
      }
    : actual;
}
