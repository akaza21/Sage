import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

import { recordDigest } from "./ade0006a-c0b-common.mjs";
import {
  deriveInstalledPayloadSet,
  derivePackageBudgetEvaluation,
  derivePackageDeltas,
  deriveParticipantBinding,
  loadAuthenticatedFrontendBaseline,
  validatePackageParticipantProvenance,
} from "./ade0006a-package-receipt.mjs";

const digest = (value) => value.repeat(64).slice(0, 64);

function participant(frontendRawBytes, candidate = false) {
  const graph = {
    cargo: [
      {
        ecosystem: "cargo",
        name: "app",
        version: "0.1.0",
        source: "workspace:control:tools/control/Cargo.toml",
        checksum: null,
        features: [],
        dependencies: [],
      },
    ],
    frontend: candidate
      ? [
          {
            ecosystem: "npm",
            name: "ui",
            version: "1.0.0",
            source: null,
            checksum: null,
            features: [],
            dependencies: [],
          },
        ]
      : [],
  };
  const summary = {
    configuration: {
      productionTauriBaseConfigSha256: digest("a"),
      productionTauriWindowsConfigSha256: digest("b"),
    },
    provenance: {
      cargoLockSha256: digest("c"),
      frontendDependencyMode: candidate
        ? "npm-package-lock-v3"
        : "static-no-package-manager",
      frontendLockSha256: candidate ? digest("d") : null,
      dependencyGraphSha256: recordDigest(graph),
      dependencyPackageCount: candidate ? 2 : 1,
      frontendRawBytes,
      frontendTreeSha256: recordDigest([
        {
          ordinal: 1,
          relativePath: "index.html",
          type: "regularFile",
          bytes: frontendRawBytes,
          sha256: digest("f"),
        },
      ]),
      dependencyGraph: graph,
      frontendMembers: [
        {
          ordinal: 1,
          relativePath: "index.html",
          type: "regularFile",
          bytes: frontendRawBytes,
          sha256: digest("f"),
        },
      ],
    },
    productionBuild: { enabledFeatures: ["custom-protocol"] },
    artifactSet: {
      entries: [
        { kind: "executable", sha256: digest("1"), bytes: 100 },
        { kind: "msi", sha256: digest("2"), bytes: 200 },
        { kind: "nsis", sha256: digest("3"), bytes: 300 },
      ],
      sha256: digest("4"),
      bytes: 600,
    },
    installers: [
      {
        kind: "msi",
        memberManifest: { sha256: digest("5"), regularFileBytes: 110 },
        installedExecutable: { sha256: digest("6"), bytes: 100 },
      },
      {
        kind: "nsis",
        memberManifest: { sha256: digest("7"), regularFileBytes: 120 },
        installedExecutable: { sha256: digest("8"), bytes: 100 },
      },
    ],
  };
  if (candidate) {
    summary.characterization = {
      executableSha256: digest("9"),
      executableBytes: 125,
      featureSet: ["acp-fixture", "custom-protocol"],
      fingerprintSha256: digest("0"),
    };
  }
  return summary;
}

function bindingReceipt() {
  return {
    source: {
      commit: digest("a"),
      tree: digest("b"),
      aggregateSha256: digest("c"),
      manifestSha256: digest("d"),
      fileCount: 10,
      bytes: 1000,
    },
    contractManifestSha256: digest("e"),
    buildIdentity: { recipe: "test" },
    platform: "windows",
    architecture: "x86_64",
    participants: {
      candidate: participant(32, true),
      control: participant(8, false),
    },
  };
}

test("installed payload set is an ordered digest of the exact installer projection", () => {
  const summary = participant(8);
  const observed = deriveInstalledPayloadSet(summary);
  assert.equal(observed.installedPayloadSetBytes, 230);
  const records = summary.installers.map((installer) => ({
    kind: installer.kind,
    memberManifestSha256: installer.memberManifest.sha256,
    installedExecutableSha256: installer.installedExecutable.sha256,
    installedExecutableBytes: installer.installedExecutable.bytes,
    installedPayloadBytes: installer.memberManifest.regularFileBytes,
  }));
  assert.equal(observed.installedPayloadSetSha256, recordDigest(records));
});

test("candidate and control bindings use only their declared scopes", () => {
  const root = path.resolve(".");
  const packageContract = JSON.parse(
    readFileSync(
      path.join(
        root,
        "docs/work-items/ADE-0006A-acp-session-foundation/contracts/package-contract-v1.json",
      ),
    ),
  );
  const evidence = JSON.parse(
    readFileSync(
      path.join(
        root,
        "docs/work-items/ADE-0006A-acp-session-foundation/contracts/evidence-policy-v1.json",
      ),
    ),
  );
  const receipt = bindingReceipt();
  const candidate = deriveParticipantBinding(receipt, "candidate");
  const control = deriveParticipantBinding(receipt, "control");
  assert.deepEqual(Object.keys(candidate), evidence.requiredCandidateBinding);
  assert.deepEqual(Object.keys(candidate), [
    ...packageContract.requiredSharedBinding,
    ...packageContract.requiredParticipantBinding,
    ...packageContract.requiredCandidateCharacterizationBinding,
  ]);
  assert.deepEqual(Object.keys(control), [
    ...packageContract.requiredSharedBinding,
    ...packageContract.requiredParticipantBinding,
  ]);
  assert.equal("characterizationExecutableSha256" in control, false);
});

test("retained provenance is recomputed and static controls cannot invent a lock", () => {
  const summary = participant(8, false);
  const frozen = {
    package: {
      observerBounds: {
        dependencyPackages: 16,
        frontendMembers: 16,
        frontendBytes: 1024,
        pathUtf8Bytes: 256,
      },
      participants: {
        control: {
          provenance: {
            cargoLock: "tools/control/Cargo.lock",
            frontendDependencyMode: "static-no-package-manager",
            frontendLock: null,
          },
        },
      },
    },
  };
  const source = {
    files: [{ path: "tools/control/Cargo.lock", sha256: digest("c") }],
  };
  const accepted = [];
  validatePackageParticipantProvenance(
    accepted,
    "control",
    summary,
    source,
    frozen,
  );
  assert.deepEqual(accepted, []);
  summary.provenance.frontendLockSha256 = digest("d");
  const rejected = [];
  validatePackageParticipantProvenance(
    rejected,
    "control",
    summary,
    source,
    frozen,
  );
  assert.ok(rejected.length > 0);
});

test("frontend bytes use the authenticated pre-ADE Sage baseline", async () => {
  const root = path.resolve(".");
  const packageContract = JSON.parse(
    readFileSync(
      path.join(
        root,
        "docs/work-items/ADE-0006A-acp-session-foundation/contracts/package-contract-v1.json",
      ),
    ),
  );
  const frontendBaseline = await loadAuthenticatedFrontendBaseline({
    sourceRoot: root,
    frozen: { package: packageContract },
  });
  assert.deepEqual(frontendBaseline, {
    workItem: "ADE-0005",
    relativePath:
      "docs/work-items/ADE-0005-project-lifecycle/evidence/curated-summary.json",
    sha256: "1f9a723991c82039f794abff1bb6137acf01bb14be9abea56d911cf12aa64dfc",
    bytes: 11106,
    sourceCommit: "b07910b9532586bf1770551348d456666a3a140b",
    sourceTree: "2ed268e29fb435c04c560d6c150f0b929c97f20e",
    frontendRawBytes: 289190,
  });

  const receipt = bindingReceipt();
  const deltas = derivePackageDeltas(
    receipt.participants.candidate,
    receipt.participants.control,
    { frontendRawBytes: 20 },
  );
  assert.equal(deltas.frontendRawBytes, 12);
  assert.equal(deltas.productionExecutableBytes, 0);
  const evaluation = derivePackageBudgetEvaluation(deltas, {
    budgets: {
      packageDelta: {
        frontendRawBytesMaximum: 16,
        productionExecutableBytesMaximum: 100,
        msiInstallerBytesMaximum: 100,
        nsisInstallerBytesMaximum: 100,
        productionArtifactSetBytesMaximum: 100,
        msiInstalledPayloadBytesMaximum: 100,
        nsisInstalledPayloadBytesMaximum: 100,
        installedPayloadSetBytesMaximum: 200,
      },
    },
  });
  assert.equal(evaluation[0].metric, "frontendRawBytes");
  assert.equal(evaluation[0].passed, true);
});
