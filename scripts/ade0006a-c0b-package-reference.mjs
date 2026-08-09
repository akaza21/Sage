import path from "node:path";

import {
  exactKeys,
  parseBoundedJson,
  recordDigest,
  sameValue,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import {
  deriveCandidateFixtureBinding,
  deriveParticipantBinding,
  validatePackageReceipt,
} from "./ade0006a-package-receipt.mjs";

const participants = ["candidate", "control"];
const installerKinds = new Set(["msi", "nsis"]);

export function deriveInstalledExecutableBinding({
  packageReceiptSha256,
  participant,
  installerKind,
  memberManifestSha256,
  installedExecutableSha256,
  installedExecutableBytes,
}) {
  return recordDigest({
    packageReceiptSha256,
    participant,
    installerKind,
    memberManifestSha256,
    installedExecutableSha256,
    installedExecutableBytes,
  });
}

export function selectPackagedObservationExecutable({
  authenticatedPackage,
  artifacts,
  participant,
}) {
  const installerKind = authenticatedPackage?.installerKind;
  const expected = authenticatedPackage?.participants?.[participant];
  const artifact =
    artifacts?.[participant]?.installedExecutables?.[installerKind];
  if (
    !participants.includes(participant) ||
    !installerKinds.has(installerKind) ||
    !artifact ||
    Object.keys(artifact).sort().join("\0") !== "bytes\0path\0sha256" ||
    typeof artifact.path !== "string" ||
    !path.isAbsolute(artifact.path) ||
    !SHA256.test(artifact.sha256 ?? "") ||
    !Number.isSafeInteger(artifact.bytes) ||
    artifact.bytes < 1 ||
    artifact.sha256 !== expected?.installedExecutableSha256 ||
    artifact.bytes !== expected?.installedExecutableBytes ||
    expected?.participant !== participant ||
    expected?.installerKind !== installerKind ||
    !SHA256.test(expected?.executableBindingSha256 ?? "") ||
    deriveInstalledExecutableBinding({
      packageReceiptSha256: authenticatedPackage.receiptSha256,
      participant,
      installerKind,
      memberManifestSha256: expected.memberManifestSha256,
      installedExecutableSha256: expected.installedExecutableSha256,
      installedExecutableBytes: expected.installedExecutableBytes,
    }) !== expected.executableBindingSha256
  ) {
    throw new Error("packaged-observation-executable-mismatch");
  }
  return Object.freeze({
    path: artifact.path,
    sha256: artifact.sha256,
    bytes: artifact.bytes,
    bindingSha256: expected.executableBindingSha256,
  });
}

function selectedParticipant(
  receipt,
  packageReceiptSha256,
  participant,
  installerKind,
) {
  const summary = receipt.participants[participant];
  const installer = summary.installers.find(
    ({ kind }) => kind === installerKind,
  );
  if (!installer) throw new Error("package-receipt-selected-installer-missing");
  const binding = {
    participant,
    installerKind,
    memberManifestSha256: installer.memberManifest.sha256,
    installedExecutableSha256: installer.installedExecutable.sha256,
    installedExecutableBytes: installer.installedExecutable.bytes,
  };
  return {
    ...binding,
    executableBindingSha256: deriveInstalledExecutableBinding({
      packageReceiptSha256,
      ...binding,
    }),
  };
}

export async function authenticatePackageReceipt({
  receiptPath,
  rawRoot,
  expectedReceiptSha256,
  installerKind,
  frozen,
}) {
  if (
    !SHA256.test(expectedReceiptSha256 ?? "") ||
    !installerKinds.has(installerKind)
  ) {
    throw new Error("package-receipt-request-invalid");
  }
  const record = parseBoundedJson(
    receiptPath,
    frozen.evidence.campaignBounds.curatedBytes,
    frozen.limits.wire,
    "Package receipt",
  );
  if (record.sha256 !== expectedReceiptSha256) {
    throw new Error("package-receipt-hash-mismatch");
  }
  const errors = await validatePackageReceipt({
    receipt: record.value,
    receiptBytes: record.buffer,
    rawRoot,
    frozen,
  });
  if (
    errors.length > 0 ||
    record.value.result !== "passed" ||
    record.value.budgetEvaluation.some(({ passed }) => passed !== true)
  ) {
    throw new Error("package-receipt-not-accepted");
  }
  const reference = {
    campaignId: record.value.campaignId,
    receiptSha256: record.sha256,
    contractManifestSha256: record.value.contractManifestSha256,
    packageContractSha256: record.value.packageContractSha256,
    sourceAggregateSha256: record.value.source.aggregateSha256,
    candidateBindingSha256: recordDigest(
      deriveParticipantBinding(record.value, "candidate"),
    ),
    installerKind,
    participants: Object.fromEntries(
      participants.map((participant) => [
        participant,
        selectedParticipant(
          record.value,
          record.sha256,
          participant,
          installerKind,
        ),
      ]),
    ),
    fixture: {
      featureSet: record.value.participants.candidate.fixture.featureSet,
      executableSha256:
        record.value.participants.candidate.fixture.executableSha256,
      executableBytes:
        record.value.participants.candidate.fixture.executableBytes,
      fixtureBindingSha256: recordDigest(
        deriveCandidateFixtureBinding(record.value),
      ),
    },
    characterization: {
      executableSha256:
        record.value.participants.candidate.characterization.executableSha256,
      executableBytes:
        record.value.participants.candidate.characterization.executableBytes,
      featureSet:
        record.value.participants.candidate.characterization.featureSet,
      buildFingerprintSha256:
        record.value.participants.candidate.characterization.fingerprintSha256,
    },
  };
  return reference;
}

export function validatePackageReference(errors, value, authenticated) {
  exactKeys(
    errors,
    value,
    [
      "campaignId",
      "receiptSha256",
      "contractManifestSha256",
      "packageContractSha256",
      "sourceAggregateSha256",
      "candidateBindingSha256",
      "installerKind",
      "participants",
      "fixture",
      "characterization",
    ],
    "packageReference",
  );
  sameValue(errors, value, authenticated, "packageReference");
}
