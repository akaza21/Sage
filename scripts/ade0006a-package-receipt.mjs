import { readdir } from "node:fs/promises";
import path from "node:path";

import {
  exactKeys,
  inspectSanitized,
  parseBoundedJson,
  recordDigest,
  safeInteger,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import {
  derivePackageBudgetEvaluation,
  derivePackageDeltas,
} from "./ade0006a-package-binding.mjs";
import { validateAuthenticatedPackageBuildIdentity } from "./ade0006a-package-build-identity-validation.mjs";
import { validatePackageParticipant } from "./ade0006a-package-participant-validation.mjs";

export {
  deriveCandidateFixtureBinding,
  deriveInstalledPayloadSet,
  derivePackageBudgetEvaluation,
  derivePackageDeltas,
  deriveParticipantBinding,
  loadAuthenticatedFrontendBaseline,
} from "./ade0006a-package-binding.mjs";
export { validatePackageParticipantProvenance } from "./ade0006a-package-provenance-validation.mjs";

const participants = ["candidate", "control"];
const installers = ["msi", "nsis"];
const gitObject = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;

export function inspectPackageReceiptSanitized(receipt, errors) {
  inspectSanitized(receipt, errors, "package-receipt", ["budgetEvaluation"]);
}

async function exactDirectory(errors, directory, expectedNames, label) {
  try {
    const entries = await readdir(directory, { withFileTypes: true });
    if (
      entries.some((entry) => !entry.isFile() && !entry.isDirectory()) ||
      entries
        .map(({ name }) => name)
        .sort()
        .join("\0") !== [...expectedNames].sort().join("\0")
    ) {
      errors.push(`${label} inventory is invalid`);
    }
  } catch {
    errors.push(`${label} is missing or unreadable`);
  }
}

function receiptAllocationIdentity(receipt, frozen) {
  const candidate = receipt?.participants?.candidate;
  const control = receipt?.participants?.control;
  if (
    !candidate?.configuration ||
    !control?.configuration ||
    !Array.isArray(candidate?.installers) ||
    candidate.installers.length !== 2
  ) {
    return undefined;
  }
  const evidence = {
    sourceCommit: receipt?.source?.commit,
    sourceTree: receipt?.source?.tree,
    sourceAggregateSha256: receipt?.source?.aggregateSha256,
    contractManifestSha256: receipt?.contractManifestSha256,
    packageContractSha256: receipt?.packageContractSha256,
    buildIdentitySha256: recordDigest(receipt?.buildIdentity),
    productionBuild: frozen.package.productionBuild,
    frontendBaseline: frozen.frontendBaseline,
    configurations: {
      candidate: {
        baseSha256: candidate.configuration.productionTauriBaseConfigSha256,
        platformSha256:
          candidate.configuration.productionTauriWindowsConfigSha256,
      },
      control: {
        baseSha256: control.configuration.productionTauriBaseConfigSha256,
        platformSha256:
          control.configuration.productionTauriWindowsConfigSha256,
      },
    },
    tools: candidate.installers.map((installer) => ({
      name: installer.kind === "msi" ? "wix-dark" : "seven-zip",
      sha256: installer.extractor?.toolSha256,
    })),
  };
  return recordDigest(evidence);
}

export async function validatePackageReceipt({
  receipt,
  receiptBytes,
  rawRoot,
  frozen,
}) {
  const errors = [];
  exactKeys(
    errors,
    receipt,
    [
      "schemaVersion",
      "workItem",
      "phase",
      "campaignId",
      "allocationIdentity",
      "startedAtUtc",
      "completedAtUtc",
      "result",
      "contractManifestSha256",
      "packageContractSha256",
      "buildIdentity",
      "platform",
      "architecture",
      "source",
      "frontendBaseline",
      "participants",
      "deltas",
      "budgetEvaluation",
      "webviewProvisioning",
      "retention",
    ],
    "receipt",
  );
  if (
    receipt?.schemaVersion !== 1 ||
    receipt?.workItem !== "ADE-0006A" ||
    receipt?.phase !== "Package" ||
    !/^[a-z0-9][a-z0-9-]{0,63}$/.test(receipt?.campaignId ?? "") ||
    !SHA256.test(receipt?.allocationIdentity ?? "") ||
    Number.isNaN(Date.parse(receipt?.startedAtUtc ?? "")) ||
    Number.isNaN(Date.parse(receipt?.completedAtUtc ?? "")) ||
    Date.parse(receipt?.completedAtUtc ?? "") <
      Date.parse(receipt?.startedAtUtc ?? "") ||
    receipt?.contractManifestSha256 !== frozen.contractManifestSha256 ||
    receipt?.packageContractSha256 !== frozen.packageContractSha256
  ) {
    errors.push("receipt header or frozen identity is invalid");
  }
  validateAuthenticatedPackageBuildIdentity(
    errors,
    receipt?.buildIdentity,
    frozen,
  );
  exactKeys(
    errors,
    receipt?.source,
    [
      "commit",
      "tree",
      "aggregateSha256",
      "manifestSha256",
      "fileCount",
      "bytes",
    ],
    "receipt.source",
  );
  if (
    receipt?.platform !== frozen.package.platform ||
    receipt?.architecture !== frozen.package.architecture ||
    !gitObject.test(receipt?.source?.commit ?? "") ||
    !gitObject.test(receipt?.source?.tree ?? "")
  ) {
    errors.push("receipt platform or source Git identity is invalid");
  }
  exactKeys(
    errors,
    receipt?.frontendBaseline,
    [
      "workItem",
      "relativePath",
      "sha256",
      "bytes",
      "sourceCommit",
      "sourceTree",
      "frontendRawBytes",
    ],
    "receipt.frontendBaseline",
  );
  if (
    JSON.stringify(receipt?.frontendBaseline) !==
    JSON.stringify(frozen.frontendBaseline)
  ) {
    errors.push("receipt frontend baseline does not match frozen evidence");
  }
  exactKeys(
    errors,
    receipt?.participants,
    participants,
    "receipt.participants",
  );
  exactKeys(
    errors,
    receipt?.retention,
    ["rawFiles", "rawBytes", "curatedFiles", "curatedBytes"],
    "receipt.retention",
  );
  exactKeys(
    errors,
    receipt?.webviewProvisioning,
    ["mode", "whenRuntimeAbsent", "networkRequiredWhenTriggered"],
    "receipt.webviewProvisioning",
  );
  await exactDirectory(
    errors,
    rawRoot,
    ["package", "source-manifest-v1.json"],
    "package raw root",
  );
  await exactDirectory(
    errors,
    path.join(rawRoot, "package"),
    participants,
    "package participant root",
  );
  for (const participant of participants) {
    await exactDirectory(
      errors,
      path.join(rawRoot, "package", participant),
      installers,
      `package ${participant} installer root`,
    );
  }
  let sourceRecord;
  try {
    sourceRecord = parseBoundedJson(
      path.join(rawRoot, "source-manifest-v1.json"),
      frozen.evidence.packageCampaignRetention.reservation.sourceManifestBytes,
      {
        ...frozen.limits.wire,
        arrayEntries:
          frozen.evidence.packageCampaignRetention.reservation
            .sourceManifestEntries,
      },
      "package source manifest",
    );
  } catch {
    errors.push("receipt source manifest is invalid");
  }
  const sourceBytes = sourceRecord?.bytes ?? 0;
  const source = sourceRecord?.value;
  if (
    sourceRecord?.sha256 !== receipt?.source?.manifestSha256 ||
    source?.aggregateSha256 !== receipt?.source?.aggregateSha256 ||
    source?.fileCount !== receipt?.source?.fileCount ||
    source?.totalBytes !== receipt?.source?.bytes
  ) {
    errors.push("receipt source manifest does not reconcile");
  }
  if (
    receiptAllocationIdentity(receipt, frozen) !== receipt.allocationIdentity
  ) {
    errors.push("receipt allocation identity does not reconcile");
  }
  for (const participant of participants) {
    await validatePackageParticipant({
      errors,
      receipt,
      participant,
      source,
      rawRoot,
      frozen,
    });
  }
  try {
    const deltas = derivePackageDeltas(
      receipt?.participants?.candidate,
      receipt?.participants?.control,
      receipt?.frontendBaseline,
    );
    const evaluation = derivePackageBudgetEvaluation(deltas, frozen);
    const result = evaluation.every(({ passed }) => passed)
      ? "passed"
      : "failed";
    if (
      JSON.stringify(receipt?.deltas) !== JSON.stringify(deltas) ||
      JSON.stringify(receipt?.budgetEvaluation) !==
        JSON.stringify(evaluation) ||
      receipt?.result !== result
    ) {
      errors.push(
        "receipt budget disposition is not derived from observations",
      );
    }
    const shardRecords = participants.flatMap((participant) =>
      receipt.participants[participant].installers.flatMap(
        (installer) => installer.memberManifest.shards,
      ),
    );
    const reservation = frozen.evidence.packageCampaignRetention.reservation;
    safeInteger(
      errors,
      receipt?.retention?.rawFiles,
      "receipt.retention.rawFiles",
      1,
      reservation.rawFiles,
    );
    safeInteger(
      errors,
      receipt?.retention?.rawBytes,
      "receipt.retention.rawBytes",
      1,
      reservation.rawBytes,
    );
    safeInteger(
      errors,
      receipt?.retention?.curatedBytes,
      "receipt.retention.curatedBytes",
      1,
      reservation.curatedBytes,
    );
    if (
      receipt?.retention?.rawFiles !== 1 + shardRecords.length ||
      receipt?.retention?.rawBytes !==
        sourceBytes +
          shardRecords.reduce((total, shard) => total + shard.bytes, 0) ||
      receipt?.retention?.curatedFiles !== 1 ||
      receipt?.retention?.curatedBytes !== receiptBytes.byteLength
    ) {
      errors.push("receipt retained evidence accounting does not reconcile");
    }
  } catch {
    errors.push("receipt observations are incomplete or malformed");
  }
  inspectPackageReceiptSanitized(receipt, errors);
  return errors;
}
