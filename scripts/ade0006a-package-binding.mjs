import path from "node:path";

import {
  exactKeys,
  parseBoundedJson,
  recordDigest,
  SHA256,
} from "./ade0006a-c0b-common.mjs";

const participants = ["candidate", "control"];
const installers = ["msi", "nsis"];
const gitObject = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;

export function loadAuthenticatedFrontendBaseline({ sourceRoot, frozen }) {
  const contract = frozen?.package?.frontendSizeBaseline;
  const errors = [];
  exactKeys(
    errors,
    contract,
    [
      "kind",
      "workItem",
      "relativePath",
      "sha256",
      "maximumEvidenceBytes",
      "sourceCommit",
      "sourceTree",
      "jsonPointer",
      "frontendRawBytes",
      "comparison",
    ],
    "frontendSizeBaseline",
  );
  if (
    errors.length > 0 ||
    contract?.kind !== "tracked-curated-evidence" ||
    contract?.workItem !== "ADE-0005" ||
    typeof contract?.relativePath !== "string" ||
    path.isAbsolute(contract.relativePath) ||
    contract.relativePath.includes("\\") ||
    contract.relativePath.split("/").includes("..") ||
    !SHA256.test(contract?.sha256 ?? "") ||
    !Number.isSafeInteger(contract?.maximumEvidenceBytes) ||
    contract.maximumEvidenceBytes < 1 ||
    !gitObject.test(contract?.sourceCommit ?? "") ||
    !gitObject.test(contract?.sourceTree ?? "") ||
    contract?.jsonPointer !== "/artifacts/freshIsolatedRebuild/frontendBytes" ||
    !Number.isSafeInteger(contract?.frontendRawBytes) ||
    contract.frontendRawBytes < 1
  ) {
    throw new Error("frontend-size-baseline-contract-invalid");
  }
  const record = parseBoundedJson(
    path.join(sourceRoot, contract.relativePath),
    contract.maximumEvidenceBytes,
    {
      jsonDepth: 32,
      jsonNodes: 16384,
      objectMembers: 256,
      arrayEntries: 4096,
      decodedKeyBytes: 256,
    },
    "frontend size baseline",
  );
  const summary = record.value;
  if (
    record.sha256 !== contract.sha256 ||
    summary?.schemaVersion !== 1 ||
    summary?.workItem !== contract.workItem ||
    summary?.sanitized !== true ||
    summary?.candidate?.implementationCommit !== contract.sourceCommit ||
    summary?.candidate?.implementationTree !== contract.sourceTree ||
    summary?.artifacts?.freshIsolatedRebuild?.frontendBytes !==
      contract.frontendRawBytes ||
    summary?.sourceAndTestGates?.freshIsolatedPackageBuildPassed !== true
  ) {
    throw new Error("frontend-size-baseline-evidence-mismatch");
  }
  return Object.freeze({
    workItem: contract.workItem,
    relativePath: contract.relativePath,
    sha256: record.sha256,
    bytes: record.bytes,
    sourceCommit: contract.sourceCommit,
    sourceTree: contract.sourceTree,
    frontendRawBytes: contract.frontendRawBytes,
  });
}

function artifact(summary, kind) {
  const value = summary.artifactSet.entries.find(
    (entry) => entry.kind === kind,
  );
  if (!value) throw new Error(`participant-${kind}-artifact-missing`);
  return value;
}

function installer(summary, kind) {
  const value = summary.installers.find((entry) => entry.kind === kind);
  if (!value) throw new Error(`participant-${kind}-installer-missing`);
  return value;
}

export function deriveInstalledPayloadSet(summary) {
  const records = installers.map((kind) => {
    const value = installer(summary, kind);
    return {
      kind,
      memberManifestSha256: value.memberManifest.sha256,
      installedExecutableSha256: value.installedExecutable.sha256,
      installedExecutableBytes: value.installedExecutable.bytes,
      installedPayloadBytes: value.memberManifest.regularFileBytes,
    };
  });
  return {
    installedPayloadSetSha256: recordDigest(records),
    installedPayloadSetBytes: records.reduce(
      (total, { installedPayloadBytes }) => total + installedPayloadBytes,
      0,
    ),
  };
}

export function deriveCandidateFixtureBinding(receipt) {
  const fixture = receipt.participants.candidate.fixture;
  return {
    featureSet: fixture.featureSet,
    executableSha256: fixture.executableSha256,
    executableBytes: fixture.executableBytes,
  };
}

export function deriveParticipantBinding(receipt, participant) {
  if (!participants.includes(participant)) {
    throw new Error("participant-binding-identity-invalid");
  }
  const summary = receipt.participants[participant];
  const msi = installer(summary, "msi");
  const nsis = installer(summary, "nsis");
  const executable = artifact(summary, "executable");
  const msiArtifact = artifact(summary, "msi");
  const nsisArtifact = artifact(summary, "nsis");
  const binding = {
    sourceCommit: receipt.source.commit,
    sourceTree: receipt.source.tree,
    sourceAggregateSha256: receipt.source.aggregateSha256,
    sourceManifestSha256: receipt.source.manifestSha256,
    sourceFileCount: receipt.source.fileCount,
    sourceBytes: receipt.source.bytes,
    contractManifestSha256: receipt.contractManifestSha256,
    buildIdentitySha256: recordDigest(receipt.buildIdentity),
    platform: receipt.platform,
    architecture: receipt.architecture,
    productionTauriBaseConfigSha256:
      summary.configuration.productionTauriBaseConfigSha256,
    productionTauriWindowsConfigSha256:
      summary.configuration.productionTauriWindowsConfigSha256,
    cargoLockSha256: summary.provenance.cargoLockSha256,
    frontendLockSha256: summary.provenance.frontendLockSha256,
    dependencyGraphSha256: summary.provenance.dependencyGraphSha256,
    dependencyPackageCount: summary.provenance.dependencyPackageCount,
    frontendRawBytes: summary.provenance.frontendRawBytes,
    frontendTreeSha256: summary.provenance.frontendTreeSha256,
    productionFeatureSet: summary.productionBuild.enabledFeatures,
    productionExecutableSha256: executable.sha256,
    productionExecutableBytes: executable.bytes,
    productionMsiSha256: msiArtifact.sha256,
    productionMsiBytes: msiArtifact.bytes,
    productionNsisSha256: nsisArtifact.sha256,
    productionNsisBytes: nsisArtifact.bytes,
    productionArtifactSetSha256: summary.artifactSet.sha256,
    productionArtifactSetBytes: summary.artifactSet.bytes,
    msiMemberManifestSha256: msi.memberManifest.sha256,
    msiInstalledExecutableSha256: msi.installedExecutable.sha256,
    msiInstalledExecutableBytes: msi.installedExecutable.bytes,
    msiInstalledPayloadBytes: msi.memberManifest.regularFileBytes,
    nsisMemberManifestSha256: nsis.memberManifest.sha256,
    nsisInstalledExecutableSha256: nsis.installedExecutable.sha256,
    nsisInstalledExecutableBytes: nsis.installedExecutable.bytes,
    nsisInstalledPayloadBytes: nsis.memberManifest.regularFileBytes,
    ...deriveInstalledPayloadSet(summary),
  };
  if (participant === "candidate") {
    Object.assign(binding, {
      characterizationExecutableSha256:
        summary.characterization.executableSha256,
      characterizationExecutableBytes: summary.characterization.executableBytes,
      characterizationFeatureSet: summary.characterization.featureSet,
    });
  }
  return binding;
}

function artifactBytes(summary, kind) {
  return artifact(summary, kind).bytes;
}

function installedBytes(summary, kind) {
  return installer(summary, kind).memberManifest.regularFileBytes;
}

export function derivePackageDeltas(candidate, control, frontendBaseline) {
  if (
    !Number.isSafeInteger(frontendBaseline?.frontendRawBytes) ||
    frontendBaseline.frontendRawBytes < 1
  ) {
    throw new Error("frontend-size-baseline-invalid");
  }
  const msiInstalledPayloadBytes =
    installedBytes(candidate, "msi") - installedBytes(control, "msi");
  const nsisInstalledPayloadBytes =
    installedBytes(candidate, "nsis") - installedBytes(control, "nsis");
  return {
    frontendRawBytes:
      candidate.provenance.frontendRawBytes - frontendBaseline.frontendRawBytes,
    productionExecutableBytes:
      artifactBytes(candidate, "executable") -
      artifactBytes(control, "executable"),
    productionMsiBytes:
      artifactBytes(candidate, "msi") - artifactBytes(control, "msi"),
    productionNsisBytes:
      artifactBytes(candidate, "nsis") - artifactBytes(control, "nsis"),
    productionArtifactSetBytes:
      candidate.artifactSet.bytes - control.artifactSet.bytes,
    msiInstalledPayloadBytes,
    nsisInstalledPayloadBytes,
    installedPayloadSetBytes:
      msiInstalledPayloadBytes + nsisInstalledPayloadBytes,
  };
}

export function derivePackageBudgetEvaluation(deltas, frozen) {
  const budgets = frozen.budgets.packageDelta;
  const ceilings = {
    frontendRawBytes: budgets.frontendRawBytesMaximum,
    productionExecutableBytes: budgets.productionExecutableBytesMaximum,
    productionMsiBytes: budgets.msiInstallerBytesMaximum,
    productionNsisBytes: budgets.nsisInstallerBytesMaximum,
    productionArtifactSetBytes: budgets.productionArtifactSetBytesMaximum,
    msiInstalledPayloadBytes: budgets.msiInstalledPayloadBytesMaximum,
    nsisInstalledPayloadBytes: budgets.nsisInstalledPayloadBytesMaximum,
    installedPayloadSetBytes: budgets.installedPayloadSetBytesMaximum,
  };
  return Object.entries(ceilings).map(([metric, maximum]) => ({
    metric,
    observed: deltas[metric],
    maximum,
    passed: deltas[metric] <= maximum,
  }));
}
