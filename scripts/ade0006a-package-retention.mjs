import { mkdir, rename, rm, writeFile } from "node:fs/promises";
import path from "node:path";

import {
  readBoundedFile,
  recordDigest,
  sha256,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import {
  optionalStat,
  packageRoots,
  projectedRaw,
  readLedger,
  reconcile,
  scanTree,
  withAdmissionLock,
  writeLedger,
} from "./ade0006a-package-retention-ledger.mjs";

const campaignIdPattern = /^[a-z0-9][a-z0-9-]{0,63}$/;

function retentionReservation(frozen) {
  const policy = frozen.evidence.packageCampaignRetention.reservation;
  const contract = frozen.package.retention.rawReservation;
  const memberSets = 2 * 2;
  const expected = {
    sourceManifestFiles: 1,
    sourceManifestBytes: 8 * 1024 * 1024,
    sourceManifestEntries: 4096,
    memberSidecarFiles:
      memberSets *
      frozen.package.observerBounds.memberManifestShardsPerInstaller,
    memberSidecarBytes:
      memberSets *
      frozen.package.observerBounds.memberManifestBytesPerInstaller,
  };
  expected.rawFiles =
    expected.sourceManifestFiles + expected.memberSidecarFiles;
  expected.rawBytes =
    expected.sourceManifestBytes + expected.memberSidecarBytes;
  if (
    contract.sourceManifestFiles !== expected.sourceManifestFiles ||
    contract.sourceManifestBytes !== expected.sourceManifestBytes ||
    contract.sourceManifestEntries !== expected.sourceManifestEntries ||
    contract.installerMemberSets !== memberSets ||
    contract.memberSidecarFiles !== expected.memberSidecarFiles ||
    contract.memberSidecarBytes !== expected.memberSidecarBytes ||
    contract.totalFiles !== expected.rawFiles ||
    contract.totalBytes !== expected.rawBytes ||
    policy.sourceManifestFiles !== expected.sourceManifestFiles ||
    policy.sourceManifestBytes !== expected.sourceManifestBytes ||
    policy.sourceManifestEntries !== expected.sourceManifestEntries ||
    policy.memberSidecarFiles !== expected.memberSidecarFiles ||
    policy.memberSidecarBytes !== expected.memberSidecarBytes ||
    policy.rawFiles !== expected.rawFiles ||
    policy.rawBytes !== expected.rawBytes
  ) {
    throw new Error("ledger-reconciliation-failed");
  }
  return policy;
}

export function packageAllocationEvidence({
  sourceRoot,
  sourceManifest,
  sourceIdentity,
  buildIdentity,
  extractors,
  frozen,
}) {
  retentionReservation(frozen);
  const maximum = frozen.package.observerBounds.executableArtifactBytes;
  const configurations = Object.fromEntries(
    ["candidate", "control"].map((participant) => {
      const profile = frozen.package.participants[participant];
      return [
        participant,
        {
          baseSha256: sha256(
            readBoundedFile(
              path.join(sourceRoot, profile.configuration.base),
              256 * 1024,
              `${participant} base config`,
            ),
          ),
          platformSha256: sha256(
            readBoundedFile(
              path.join(sourceRoot, profile.configuration.platformOverlay),
              256 * 1024,
              `${participant} platform config`,
            ),
          ),
        },
      ];
    }),
  );
  const tools = [
    {
      name: "wix-dark",
      sha256: sha256(readBoundedFile(extractors.msi, maximum, "wix dark")),
    },
    {
      name: "seven-zip",
      sha256: sha256(readBoundedFile(extractors.nsis, maximum, "7-Zip")),
    },
  ];
  const evidence = {
    sourceCommit: sourceIdentity.commit,
    sourceTree: sourceIdentity.tree,
    sourceAggregateSha256: sourceManifest.aggregateSha256,
    contractManifestSha256: frozen.contractManifestSha256,
    packageContractSha256: frozen.packageContractSha256,
    buildIdentitySha256: recordDigest(buildIdentity),
    productionBuild: frozen.package.productionBuild,
    frontendBaseline: frozen.frontendBaseline,
    configurations,
    tools,
  };
  return { evidence, allocationIdentity: recordDigest(evidence) };
}

export async function reservePackageCampaign({
  sourceRoot,
  campaignId,
  allocationIdentity,
  sourceAggregateSha256,
  startedAtUtc,
  frozen,
}) {
  if (
    !campaignIdPattern.test(campaignId ?? "") ||
    !SHA256.test(allocationIdentity ?? "") ||
    !SHA256.test(sourceAggregateSha256 ?? "") ||
    Number.isNaN(Date.parse(startedAtUtc ?? ""))
  ) {
    throw new Error("campaign-id-conflict");
  }
  return withAdmissionLock(sourceRoot, frozen, async (packageRoot) => {
    const ledger = await readLedger(packageRoot, frozen);
    const actual = await reconcile(sourceRoot, packageRoot, ledger, frozen);
    if (
      ledger.entries.some((entry) => entry.campaignId === campaignId) ||
      (await optionalStat(path.join(packageRoot, campaignId)))
    ) {
      throw new Error("campaign-id-conflict");
    }
    const reservation = retentionReservation(frozen);
    const campaignBounds = frozen.evidence.campaignBounds;
    const identities = new Set(
      ledger.entries.map(({ allocationIdentity: identity }) => identity),
    );
    const sameIdentity = ledger.entries.filter(
      (entry) => entry.allocationIdentity === allocationIdentity,
    );
    const sameIdentityBytes = sameIdentity.reduce(
      (total, entry) =>
        total + projectedRaw(entry, actual.get(entry.campaignId)).bytes,
      0,
    );
    const global = await scanTree(
      packageRoots(sourceRoot).evidenceRoot,
      campaignBounds,
    );
    const activeHeadroom = ledger.entries.reduce(
      (total, entry) => {
        const observed = actual.get(entry.campaignId);
        const projected = projectedRaw(entry, observed);
        return {
          files: total.files + projected.files - observed.files,
          bytes: total.bytes + projected.bytes - observed.bytes,
        };
      },
      { files: 0, bytes: 0 },
    );
    if (
      reservation.rawFiles > campaignBounds.rawFiles ||
      reservation.rawBytes > campaignBounds.rawBytes ||
      reservation.curatedFiles !== 1 ||
      reservation.curatedBytes > campaignBounds.curatedBytes ||
      sameIdentity.length + 1 > campaignBounds.campaignsPerCandidateIdentity ||
      sameIdentityBytes + reservation.rawBytes >
        campaignBounds.rawBytesPerCandidateIdentity ||
      (!identities.has(allocationIdentity) &&
        identities.size + 1 > campaignBounds.retainedCandidateIdentities) ||
      global.files + activeHeadroom.files + reservation.rawFiles + 1 >
        campaignBounds.globalFiles ||
      global.bytes +
        activeHeadroom.bytes +
        reservation.rawBytes +
        frozen.evidence.packageCampaignRetention.ledgerBytes >
        campaignBounds.globalBytes ||
      ledger.entries.length + 1 >
        frozen.evidence.packageCampaignRetention.ledgerEntries
    ) {
      throw new Error("capacity-rejected");
    }
    const entry = {
      campaignId,
      allocationIdentity,
      sourceAggregateSha256,
      contractManifestSha256: frozen.contractManifestSha256,
      packageContractSha256: frozen.packageContractSha256,
      state: "active",
      startedAtUtc,
      completedAtUtc: null,
      reservedRawFiles: reservation.rawFiles,
      reservedRawBytes: reservation.rawBytes,
      actualRawFiles: null,
      actualRawBytes: null,
      curatedReceipt: null,
      failureCode: null,
    };
    ledger.entries.push(entry);
    await mkdir(path.join(packageRoot, campaignId));
    try {
      await writeLedger(packageRoot, ledger, frozen);
    } catch (error) {
      await rm(path.join(packageRoot, campaignId), { recursive: true });
      throw error;
    }
    return {
      campaignRoot: path.join(packageRoot, campaignId),
      rawRoot: path.join(packageRoot, campaignId, "raw"),
      entry,
    };
  });
}

async function settlePackageCampaign({
  sourceRoot,
  campaignId,
  allocationIdentity,
  completedAtUtc,
  frozen,
  state,
  failureCode = null,
  receipt,
  receiptBytes,
}) {
  return withAdmissionLock(sourceRoot, frozen, async (packageRoot) => {
    const ledger = await readLedger(packageRoot, frozen);
    const actualByCampaign = await reconcile(
      sourceRoot,
      packageRoot,
      ledger,
      frozen,
    );
    const entry = ledger.entries.find(
      (candidate) => candidate.campaignId === campaignId,
    );
    if (
      !entry ||
      entry.state !== "active" ||
      entry.allocationIdentity !== allocationIdentity ||
      Number.isNaN(Date.parse(completedAtUtc ?? ""))
    ) {
      throw new Error("ledger-reconciliation-failed");
    }
    const actual = actualByCampaign.get(campaignId);
    if (
      actual.files > entry.reservedRawFiles ||
      actual.bytes > entry.reservedRawBytes
    ) {
      throw new Error("capacity-rejected");
    }
    let receiptPath;
    if (receipt) {
      if (
        receipt.campaignId !== campaignId ||
        receipt.allocationIdentity !== allocationIdentity ||
        receipt.retention.rawFiles !== actual.files ||
        receipt.retention.rawBytes !== actual.bytes ||
        receipt.retention.curatedFiles !== 1 ||
        receipt.retention.curatedBytes !== receiptBytes.byteLength
      ) {
        throw new Error("ledger-reconciliation-failed");
      }
      entry.curatedReceipt = {
        relativePath: `docs/work-items/ADE-0006A-acp-session-foundation/evidence/package/${campaignId}.json`,
        sha256: sha256(receiptBytes),
        bytes: receiptBytes.byteLength,
      };
      const { curatedRoot } = packageRoots(sourceRoot);
      await mkdir(curatedRoot, { recursive: true });
      receiptPath = path.join(curatedRoot, `${campaignId}.json`);
      const nextReceiptPath = path.join(curatedRoot, `${campaignId}.next.json`);
      if (
        (await optionalStat(receiptPath)) ||
        (await optionalStat(nextReceiptPath))
      ) {
        throw new Error("ledger-reconciliation-failed");
      }
      await writeFile(nextReceiptPath, receiptBytes, { flag: "wx" });
      try {
        await rename(nextReceiptPath, receiptPath);
      } catch (error) {
        await rm(nextReceiptPath, { force: true });
        throw error;
      }
    }
    entry.state = state;
    entry.completedAtUtc = completedAtUtc;
    entry.actualRawFiles = actual.files;
    entry.actualRawBytes = actual.bytes;
    entry.failureCode = failureCode;
    try {
      await writeLedger(packageRoot, ledger, frozen);
    } catch (error) {
      if (receiptPath) {
        try {
          await rm(receiptPath);
        } catch (rollbackError) {
          throw new Error("ledger-reconciliation-failed", {
            cause: rollbackError,
          });
        }
      }
      throw error;
    }
    return entry;
  });
}

export async function finalizePackageCampaign(options) {
  const state =
    options.receipt.result === "passed"
      ? "completed-passed"
      : "completed-failed";
  return settlePackageCampaign({ ...options, state });
}

export async function failPackageCampaign(options) {
  if (
    !options.frozen.package.retention.collectionFailureCodes.includes(
      options.failureCode,
    )
  ) {
    throw new Error("ledger-reconciliation-failed");
  }
  return settlePackageCampaign({
    ...options,
    state: "failed-unsummarized",
  });
}

export function classifyPackageFailure(error) {
  const message = String(error?.message ?? "");
  if (/fixture-build|production-build/.test(message)) return "build-failed";
  if (/extractor|inspection|member|bundle-type/.test(message)) {
    return "inspection-failed";
  }
  if (message === "source-changed-during-package-campaign") {
    return "source-drift";
  }
  if (/receipt|sidecar|budget disposition/.test(message)) {
    return "receipt-validation-failed";
  }
  return "collection-failed";
}
