import { mkdir } from "node:fs/promises";
import path from "node:path";

import {
  inspectSanitized,
  parseBoundedJson,
  recordDigest,
  SHA256,
} from "./ade0006a-c0b-common.mjs";
import {
  buildCandidateCharacterization,
  buildDiagnosticFixture,
  buildProductionPackage,
  observeCandidateCharacterization,
  observeProductionBuild,
} from "./ade0006a-package-build.mjs";
import { inspectPackageInstaller } from "./ade0006a-package-extract.mjs";
import { hashBoundedFile } from "./ade0006a-package-members.mjs";
import { collectPackageParticipantProvenance } from "./ade0006a-package-provenance.mjs";
import { snapshotPackageArtifact } from "./ade0006a-package-snapshot.mjs";

async function fileEvidence(filePath, maximumBytes) {
  const evidence = await hashBoundedFile(filePath, maximumBytes);
  return { sha256: evidence.sha256, bytes: evidence.bytes };
}

function assertPackageObservation(summary, frozen) {
  const errors = [];
  const expected = frozen.package.fixtureExclusion;
  for (const installer of summary.installers) {
    if (
      installer.executableDerivation.sourceExecutableSha256 !==
        summary.artifactSet.entries[0].sha256 ||
      installer.executableDerivation.installedExecutableSha256 !==
        installer.installedExecutable.sha256 ||
      installer.fixtureBasenameCount !==
        expected.extractedMemberBasenameCountPerInstaller ||
      installer.fixtureHashMatchCount !==
        (summary.participant === expected.participant
          ? expected.extractedPayloadHashMatchesFixturePerInstaller
          : null) ||
      installer.bundledWebviewPayloadCount !==
        frozen.package.webviewProvisioning.bundledPayloadCount ||
      installer.bundledWebviewPayloadBytes !==
        frozen.package.webviewProvisioning.bundledPayloadBytes
    ) {
      errors.push(`${installer.kind}-inspection-failed`);
    }
  }
  inspectSanitized(summary, errors, "package-observer");
  if (errors.length > 0) throw new Error(errors.join(","));
}

export async function observePackageParticipant({
  sourceRoot,
  participant,
  rawRoot,
  scratchRoot,
  buildTools,
  extractors,
  frozen,
}) {
  const profile = frozen.package.participants[participant];
  const participantMembers = path.join(rawRoot, "package", participant);
  const snapshotRoot = path.join(scratchRoot, "artifact-snapshots");
  await mkdir(snapshotRoot, { recursive: false });
  let fixtureExecutable;
  let characterization;
  if (participant === frozen.package.fixtureExclusion.participant) {
    const builtFixture = await buildDiagnosticFixture({
      sourceRoot,
      targetDirectory: path.join(scratchRoot, "fixture-target"),
      tools: buildTools,
      frozen,
    });
    fixtureExecutable = (
      await snapshotPackageArtifact({
        sourcePath: builtFixture,
        destinationPath: path.join(snapshotRoot, "fixture.exe"),
        maximumBytes: frozen.package.observerBounds.executableArtifactBytes,
        rejectHardLinks: false,
      })
    ).path;
    const characterizationTarget = await buildCandidateCharacterization({
      sourceRoot,
      targetDirectory: path.join(scratchRoot, "characterization-target"),
      tools: buildTools,
      frozen,
    });
    characterization = await observeCandidateCharacterization({
      targetDirectory: characterizationTarget,
      frozen,
    });
    const characterizationSnapshot = await snapshotPackageArtifact({
      sourcePath: characterization.executablePath,
      destinationPath: path.join(snapshotRoot, "characterization.exe"),
      maximumBytes: frozen.package.observerBounds.executableArtifactBytes,
      rejectHardLinks: false,
    });
    if (
      characterizationSnapshot.sha256 !==
        characterization.summary.executableSha256 ||
      characterizationSnapshot.bytes !==
        characterization.summary.executableBytes
    ) {
      throw new Error("characterization-artifact-changed-before-snapshot");
    }
    characterization.executablePath = characterizationSnapshot.path;
  }
  const productionTarget = await buildProductionPackage({
    sourceRoot,
    participant,
    targetDirectory: path.join(scratchRoot, "production-target"),
    tools: buildTools,
    frozen,
  });
  const productionBuild = await observeProductionBuild({
    participant,
    targetDirectory: productionTarget,
    frozen,
  });
  const provenance = await collectPackageParticipantProvenance({
    sourceRoot,
    participant,
    buildTools,
    frozen,
  });
  const extractedRoot = path.join(scratchRoot, "extracted");
  await mkdir(extractedRoot, { recursive: false });
  const artifacts = [];
  const artifactPaths = {};
  for (const [index, entry] of profile.artifacts.entries()) {
    const sourcePath = path.join(productionTarget, entry.relativePath);
    const maximum = frozen.package.observerBounds[`${entry.kind}ArtifactBytes`];
    const snapshot = await snapshotPackageArtifact({
      sourcePath,
      destinationPath: path.join(
        snapshotRoot,
        `${String(index + 1).padStart(2, "0")}-${path.basename(entry.relativePath)}`,
      ),
      maximumBytes: maximum,
      rejectHardLinks: false,
    });
    artifactPaths[entry.kind] = snapshot.path;
    artifacts.push({
      ...entry,
      sha256: snapshot.sha256,
      bytes: snapshot.bytes,
    });
  }
  const releaseExecutable = artifactPaths.executable;
  const fixture = fixtureExecutable
    ? await fileEvidence(
        fixtureExecutable,
        frozen.package.observerBounds.executableArtifactBytes,
      )
    : undefined;
  const installers = [];
  const installedExecutables = {};
  for (const kind of ["msi", "nsis"]) {
    const inspected = await inspectPackageInstaller({
      kind,
      artifact: artifactPaths[kind],
      extractor: extractors[kind],
      extractionRoot: extractedRoot,
      memberRoot: participantMembers,
      participant,
      releaseExecutable,
      fixtureExecutable,
      frozen,
      sourceRoot,
      environment: buildTools.toolEnvironment,
    });
    installers.push(inspected.evidence);
    installedExecutables[kind] = inspected.installedExecutablePath;
  }
  const baseConfig = parseBoundedJson(
    path.join(sourceRoot, profile.configuration.base),
    256 * 1024,
    frozen.limits.wire,
    "Tauri base config",
  );
  const windowsConfig = parseBoundedJson(
    path.join(sourceRoot, profile.configuration.platformOverlay),
    256 * 1024,
    frozen.limits.wire,
    "Tauri Windows config",
  );
  const metadataDeclarationCount = [
    ...(baseConfig.value.bundle?.resources ?? []),
    ...(baseConfig.value.bundle?.externalBin ?? []),
    ...(windowsConfig.value.bundle?.resources ?? []),
    ...(windowsConfig.value.bundle?.externalBin ?? []),
  ].length;
  const summary = {
    participant,
    configuration: {
      productionTauriBaseConfigSha256: baseConfig.sha256,
      productionTauriWindowsConfigSha256: windowsConfig.sha256,
      metadataDeclarationCount,
    },
    productionBuild,
    provenance,
    artifactSet: {
      entries: artifacts,
      bytes: artifacts.reduce((total, artifact) => total + artifact.bytes, 0),
      sha256: recordDigest(artifacts),
    },
    installers,
  };
  if (fixture) {
    summary.configuration.productionFeatureSetContainsFixture = false;
    summary.fixture = {
      featureSet: [frozen.package.fixtureExclusion.canonicalFeature],
      executableSha256: fixture.sha256,
      executableBytes: fixture.bytes,
    };
    summary.characterization = characterization.summary;
  }
  if (
    metadataDeclarationCount !==
      frozen.package.fixtureExclusion.metadataDeclarationCount ||
    summary.artifactSet.bytes >
      frozen.package.observerBounds.artifactSetBytes ||
    !SHA256.test(summary.artifactSet.sha256)
  ) {
    throw new Error("package-summary-invalid");
  }
  assertPackageObservation(summary, frozen);
  return {
    summary,
    ephemeral: {
      releaseExecutable,
      fixtureExecutable: fixtureExecutable ?? null,
      characterizationExecutable: characterization?.executablePath ?? null,
      installerArtifacts: {
        msi: artifactPaths.msi,
        nsis: artifactPaths.nsis,
      },
      installedExecutables,
    },
  };
}
