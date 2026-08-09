import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { recordDigest, sha256 } from "./ade0006a-c0b-common.mjs";
import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import { validatePackageParticipant } from "./ade0006a-package-participant-validation.mjs";

const frozen = loadFrozenContracts(path.resolve("."));
const hash = (value) => createHash("sha256").update(value).digest("hex");

function artifact(kind, relativePath, value) {
  return {
    kind,
    relativePath,
    sha256: hash(value),
    bytes: Buffer.byteLength(value),
  };
}

function writeInstallerSidecar(rawRoot, participant, kind, logicalPath) {
  const records = [
    {
      ordinal: 1,
      relativePath: logicalPath,
      type: "regularFile",
      bytes: 10,
      sha256: hash(`${participant}-${kind}-installed`),
    },
  ];
  const document = {
    schemaVersion: 1,
    participant,
    installer: kind,
    shardOrdinal: 1,
    firstRecordOrdinal: 1,
    records,
  };
  const bytes = Buffer.from(JSON.stringify(document));
  const directory = path.join(rawRoot, "package", participant, kind);
  mkdirSync(directory, { recursive: true });
  writeFileSync(path.join(directory, "members-0001.json"), bytes);
  return {
    records,
    shard: {
      ordinal: 1,
      firstRecordOrdinal: 1,
      recordCount: 1,
      bytes: bytes.byteLength,
      sha256: sha256(bytes),
    },
  };
}

function provenance(participant) {
  const cargoLockSha256 = hash(`${participant}-cargo-lock`);
  const candidate = participant === "candidate";
  const frontendLockSha256 = candidate ? hash("frontend-lock") : null;
  const dependencyGraph = { cargo: [], frontend: [] };
  const frontendMembers = [
    {
      ordinal: 1,
      relativePath: "index.html",
      type: "regularFile",
      bytes: 1,
      sha256: hash("x"),
    },
  ];
  return {
    summary: {
      cargoLockSha256,
      frontendDependencyMode: candidate
        ? "npm-package-lock-v3"
        : "static-no-package-manager",
      frontendLockSha256,
      dependencyGraphSha256: recordDigest(dependencyGraph),
      dependencyPackageCount: 0,
      frontendRawBytes: 1,
      frontendTreeSha256: recordDigest(frontendMembers),
      dependencyGraph,
      frontendMembers,
    },
    sourceFiles: [
      {
        path: frozen.package.participants[participant].provenance.cargoLock,
        sha256: cargoLockSha256,
      },
      ...(candidate
        ? [
            {
              path: frozen.package.participants.candidate.provenance
                .frontendLock,
              sha256: frontendLockSha256,
            },
          ]
        : []),
    ],
  };
}

function participantFixture(rawRoot, participant = "candidate") {
  const profile = frozen.package.participants[participant];
  const entries = profile.artifacts.map((entry) =>
    artifact(entry.kind, entry.relativePath, `${participant}-${entry.kind}`),
  );
  const installers = ["msi", "nsis"].map((kind) => {
    const logicalPath = profile.installedExecutableLogicalPaths[kind];
    const { records, shard } = writeInstallerSidecar(
      rawRoot,
      participant,
      kind,
      logicalPath,
    );
    const installed = records[0];
    const packageArtifact = entries.find((entry) => entry.kind === kind);
    return {
      kind,
      artifactSha256: packageArtifact.sha256,
      artifactBytes: packageArtifact.bytes,
      extractor: {
        tool: kind === "msi" ? "wix-dark" : "seven-zip",
        toolSha256: hash(`${kind}-extractor`),
        exitCode: 0,
        stdoutBytes: 0,
        stderrBytes: 0,
      },
      executableDerivation: {
        algorithm: frozen.package.installedExecutableIdentity.algorithm,
        sourceMarkerOffset: 2,
        sourceMarkerOccurrences: 1,
        targetMarkerOccurrences: 1,
        sourceExecutableSha256: entries[0].sha256,
        expectedInstalledExecutableSha256: installed.sha256,
        installedExecutableSha256: installed.sha256,
        installedExecutableBytes: installed.bytes,
      },
      memberManifest: {
        extractedRecordCount: 1,
        extractedRegularFileCount: 1,
        extractedRegularFileBytes: installed.bytes,
        recordCount: 1,
        regularFileCount: 1,
        regularFileBytes: installed.bytes,
        pathUtf8Bytes: Buffer.byteLength(logicalPath),
        shardCount: 1,
        bytes: shard.bytes,
        sha256: recordDigest(records),
        shards: [shard],
      },
      executableMapping: {
        method:
          frozen.package.inspection.installedPayloadProjection[kind].method,
        metadataSha256: kind === "msi" ? hash("msi-metadata") : null,
        mappedFileCount: 1,
        sha256: hash(`${kind}-mapping`),
      },
      installedExecutable: {
        logicalRelativePath: logicalPath,
        extractedRelativePath: logicalPath,
        bytes: installed.bytes,
        sha256: installed.sha256,
      },
      fixtureBasenameCount: 0,
      fixtureHashMatchCount: participant === "candidate" ? 0 : null,
      bundledWebviewPayloadCount: 0,
      bundledWebviewPayloadBytes: 0,
    };
  });
  const observedProvenance = provenance(participant);
  const summary = {
    participant,
    configuration: {
      productionTauriBaseConfigSha256: hash(`${participant}-base-config`),
      productionTauriWindowsConfigSha256: hash(`${participant}-windows-config`),
      metadataDeclarationCount: 0,
      ...(participant === "candidate"
        ? { productionFeatureSetContainsFixture: false }
        : {}),
    },
    productionBuild: {
      profile: frozen.package.productionBuild.profile,
      defaultFeaturesEnabled:
        frozen.package.productionBuild.defaultFeaturesEnabled,
      enabledFeatures: frozen.package.productionBuild.enabledFeatures,
      fingerprintSha256: hash(`${participant}-production-fingerprint`),
    },
    provenance: observedProvenance.summary,
    artifactSet: {
      entries,
      sha256: recordDigest(entries),
      bytes: entries.reduce((total, entry) => total + entry.bytes, 0),
    },
    installers,
  };
  if (participant === "candidate") {
    summary.characterization = {
      executableSha256: hash("characterization"),
      executableBytes: 16,
      featureSet: frozen.package.characterizationBuild.enabledFeatures,
      defaultFeaturesEnabled:
        frozen.package.characterizationBuild.defaultFeaturesEnabled,
      fingerprintSha256: hash("characterization-fingerprint"),
    };
    summary.fixture = {
      featureSet: [frozen.package.fixtureExclusion.canonicalFeature],
      executableSha256: hash("fixture"),
      executableBytes: 7,
    };
  }
  const receipt = {
    source: {
      commit: "1".repeat(40),
      tree: "2".repeat(40),
      aggregateSha256: hash("source"),
      manifestSha256: hash("manifest"),
      fileCount: observedProvenance.sourceFiles.length,
      bytes: 100,
    },
    contractManifestSha256: frozen.contractManifestSha256,
    buildIdentity: { recipe: "test" },
    platform: frozen.package.platform,
    architecture: frozen.package.architecture,
    participants: { [participant]: summary },
  };
  return {
    receipt,
    source: { files: observedProvenance.sourceFiles },
    summary,
  };
}

async function errorsFor(value, rawRoot) {
  const errors = [];
  await validatePackageParticipant({
    errors,
    receipt: value.receipt,
    participant: "candidate",
    source: value.source,
    rawRoot,
    frozen,
  });
  return errors;
}

test("participant receipt binds each inspected installer to its exact artifact", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-participant-"));
  try {
    const value = participantFixture(root);
    assert.deepEqual(await errorsFor(value, root), []);
    value.summary.installers[0].artifactSha256 = hash("different-msi");
    assert.ok(
      (await errorsFor(value, root)).some((error) =>
        error.includes("msi artifact does not match artifactSet"),
      ),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("candidate fixture and extractor evidence are exact and bounded", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-participant-"));
  try {
    const value = participantFixture(root);
    value.summary.fixture.featureSet = ["not-the-frozen-fixture"];
    value.summary.installers[1].extractor.exitCode = 1;
    value.summary.installers[1].extractor.stdoutBytes =
      frozen.package.observerBounds.toolStdoutBytes + 1;
    const errors = await errorsFor(value, root);
    assert.ok(errors.some((error) => error.includes("fixture is invalid")));
    assert.ok(errors.some((error) => error.includes("extractor is invalid")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
