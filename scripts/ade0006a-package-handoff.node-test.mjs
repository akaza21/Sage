import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  cpSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  authenticatePackageArtifactHandoff,
  deliverPackageArtifactHandoff,
} from "./ade0006a-package-handoff.mjs";
import { buildSourceManifest } from "./generate-source-manifest.mjs";
import { runAuthenticatedInstalledSmoke } from "./beta0001-installed-smoke-handoff.mjs";
import { INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS } from "./beta0001-installed-smoke-source-closure.mjs";

const participants = ["candidate", "control"];
const installers = ["msi", "nsis"];

function digest(value) {
  const bytes = Buffer.from(value);
  return {
    bytes: bytes.byteLength,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function fixture() {
  const scratchRoot = mkdtempSync(path.join(tmpdir(), "sage-handoff-"));
  const sourceRoot = path.join(scratchRoot, "source");
  mkdirSync(sourceRoot);
  writeFileSync(path.join(sourceRoot, "marker.txt"), "staged-source");
  const sourceManifest = buildSourceManifest(sourceRoot, {
    files: ["marker.txt"],
    directories: [],
  });
  const gitPath = path.join(scratchRoot, "git.exe");
  writeFileSync(gitPath, "staged-git");
  const powershellPath = path.join(scratchRoot, "powershell.exe");
  writeFileSync(powershellPath, "prior-powershell");
  const nodePath = path.join(scratchRoot, "node.exe");
  writeFileSync(nodePath, "prior-node");
  const artifacts = {};
  const receipt = {
    result: "passed",
    source: {
      aggregateSha256: sourceManifest.aggregateSha256,
      manifestSha256: digest(JSON.stringify(sourceManifest)).sha256,
      fileCount: sourceManifest.fileCount,
      bytes: sourceManifest.totalBytes,
    },
    buildIdentity: {
      git: { executableSha256: digest("staged-git").sha256 },
      node: { executableSha256: digest("prior-node").sha256 },
      powershell: {
        executableSha256: digest("prior-powershell").sha256,
      },
    },
    participants: {},
  };
  for (const participant of participants) {
    const root = path.join(scratchRoot, participant);
    mkdirSync(root);
    const releaseExecutable = path.join(root, "release.exe");
    const release = `${participant}-release`;
    writeFileSync(releaseExecutable, release);
    const installerArtifacts = {};
    const installedExecutables = {};
    const receiptInstallers = [];
    for (const installer of installers) {
      const installerArtifact = path.join(root, `${installer}-installer.exe`);
      const installerBytes = `${participant}-${installer}-installer`;
      writeFileSync(installerArtifact, installerBytes);
      installerArtifacts[installer] = installerArtifact;
      const installedExecutable = path.join(root, `${installer}.exe`);
      const installed = `${participant}-${installer}-installed`;
      writeFileSync(installedExecutable, installed);
      installedExecutables[installer] = installedExecutable;
      receiptInstallers.push({
        kind: installer,
        installedExecutable: digest(installed),
      });
    }
    let fixtureExecutable = null;
    let characterizationExecutable = null;
    if (participant === "candidate") {
      fixtureExecutable = path.join(root, "fixture.exe");
      writeFileSync(fixtureExecutable, "characterization-fixture");
      characterizationExecutable = path.join(root, "characterization.exe");
      writeFileSync(characterizationExecutable, "characterization-app");
    }
    artifacts[participant] = {
      releaseExecutable,
      fixtureExecutable,
      characterizationExecutable,
      installerArtifacts,
      installedExecutables,
    };
    receipt.participants[participant] = {
      artifactSet: {
        entries: [
          { kind: "executable", ...digest(release) },
          { kind: "msi", ...digest(`${participant}-msi-installer`) },
          { kind: "nsis", ...digest(`${participant}-nsis-installer`) },
        ],
      },
      installers: receiptInstallers,
      ...(fixtureExecutable
        ? {
            fixture: {
              executableSha256: digest("characterization-fixture").sha256,
              executableBytes: digest("characterization-fixture").bytes,
            },
            characterization: {
              executableSha256: digest("characterization-app").sha256,
              executableBytes: digest("characterization-app").bytes,
            },
          }
        : {}),
    };
  }
  return {
    scratchRoot,
    sourceRoot,
    sourceManifest,
    artifacts,
    receipt,
    git: { path: gitPath, ...digest("staged-git") },
    node: { path: nodePath, ...digest("prior-node") },
    powershell: {
      path: powershellPath,
      ...digest("prior-powershell"),
    },
  };
}

test("artifact handoff re-authenticates every executable against the receipt", async () => {
  const value = fixture();
  try {
    const authenticated = await authenticatePackageArtifactHandoff({
      scratchRoot: value.scratchRoot,
      artifacts: value.artifacts,
      receipt: value.receipt,
      maximumExecutableBytes: 1024,
      maximumInstallerBytes: 1024,
    });
    assert.deepEqual(Object.keys(authenticated), participants);
    assert.deepEqual(
      Object.keys(authenticated.candidate.installedExecutables),
      installers,
    );
    assert.equal(
      authenticated.candidate.installerArtifacts.nsis.sha256,
      value.receipt.participants.candidate.artifactSet.entries[2].sha256,
    );
    assert.equal(
      authenticated.candidate.installedExecutables.nsis.sha256,
      value.receipt.participants.candidate.installers[1].installedExecutable
        .sha256,
    );
    assert.equal(
      authenticated.candidate.fixtureExecutable.sha256,
      value.receipt.participants.candidate.fixture.executableSha256,
    );
    assert.equal(
      authenticated.candidate.characterizationExecutable.sha256,
      value.receipt.participants.candidate.characterization.executableSha256,
    );
    assert.equal(authenticated.control.fixtureExecutable, null);
    assert.equal(authenticated.control.characterizationExecutable, null);
  } finally {
    rmSync(value.scratchRoot, { recursive: true, force: true });
  }
});

test("artifact handoff rejects changed or out-of-scratch executables", async () => {
  const value = fixture();
  const outsideRoot = mkdtempSync(path.join(tmpdir(), "sage-handoff-outside-"));
  try {
    writeFileSync(value.artifacts.candidate.installerArtifacts.nsis, "changed");
    await assert.rejects(
      authenticatePackageArtifactHandoff({
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-candidate-nsis-installer-mismatch/,
    );
    writeFileSync(
      value.artifacts.candidate.installerArtifacts.nsis,
      "candidate-nsis-installer",
    );

    writeFileSync(
      value.artifacts.candidate.installedExecutables.msi,
      "changed",
    );
    await assert.rejects(
      authenticatePackageArtifactHandoff({
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-candidate-msi-mismatch/,
    );
    writeFileSync(
      value.artifacts.candidate.installedExecutables.msi,
      "candidate-msi-installed",
    );

    writeFileSync(
      value.artifacts.candidate.characterizationExecutable,
      "changed",
    );
    await assert.rejects(
      authenticatePackageArtifactHandoff({
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-candidate-characterization-mismatch/,
    );
    writeFileSync(
      value.artifacts.candidate.characterizationExecutable,
      "characterization-app",
    );

    const outside = path.join(outsideRoot, "control.exe");
    writeFileSync(outside, "control-release");
    value.artifacts.control.releaseExecutable = outside;
    await assert.rejects(
      authenticatePackageArtifactHandoff({
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-control-release-outside-scratch/,
    );
  } finally {
    rmSync(value.scratchRoot, { recursive: true, force: true });
    rmSync(outsideRoot, { recursive: true, force: true });
  }
});

test("artifact delivery skips failed receipts and awaits an isolated passing payload", async () => {
  const value = fixture();
  const originalRoot = mkdtempSync(
    path.join(tmpdir(), "sage-handoff-original-"),
  );
  const originalMarker = path.join(originalRoot, "marker.txt");
  writeFileSync(originalMarker, "original-source");
  try {
    let calls = 0;
    value.receipt.result = "failed";
    assert.equal(
      await deliverPackageArtifactHandoff({
        callback: async () => {
          calls += 1;
        },
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        receiptBytes: Buffer.from("failed-receipt"),
        rawRoot: "retained-raw-root",
        frozen: { contract: "frozen" },
        sourceRoot: value.sourceRoot,
        sourceManifest: value.sourceManifest,
        git: value.git,
        node: value.node,
        powershell: value.powershell,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      false,
    );
    assert.equal(calls, 0);

    value.receipt.result = "passed";
    const receiptBytes = Buffer.from("passing-receipt");
    let completed = false;
    assert.equal(
      await deliverPackageArtifactHandoff({
        callback: async (payload) => {
          calls += 1;
          writeFileSync(originalMarker, "post-finalization-change");
          assert.equal(payload.sourceRoot, value.sourceRoot);
          assert.deepEqual(payload.sourceManifest, value.sourceManifest);
          assert.equal(
            readFileSync(path.join(payload.sourceRoot, "marker.txt"), "utf8"),
            "staged-source",
          );
          assert.equal(payload.tools.git.sha256, value.git.sha256);
          assert.equal(payload.tools.git.bytes, value.git.bytes);
          assert.equal(
            payload.tools.powershell.sha256,
            value.powershell.sha256,
          );
          payload.receipt.result = "mutated";
          payload.receiptBytes[0] = 0;
          await Promise.resolve();
          completed = true;
        },
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        receiptBytes,
        rawRoot: "retained-raw-root",
        frozen: { contract: "frozen" },
        sourceRoot: value.sourceRoot,
        sourceManifest: value.sourceManifest,
        git: value.git,
        node: value.node,
        powershell: value.powershell,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      true,
    );
    assert.equal(calls, 1);
    assert.equal(completed, true);
    assert.equal(value.receipt.result, "passed");
    assert.equal(receiptBytes.toString(), "passing-receipt");

    writeFileSync(value.powershell.path, "changed-powershell");
    await assert.rejects(
      deliverPackageArtifactHandoff({
        callback: async () => {
          calls += 1;
        },
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        receiptBytes,
        rawRoot: "retained-raw-root",
        frozen: { contract: "frozen" },
        sourceRoot: value.sourceRoot,
        sourceManifest: value.sourceManifest,
        git: value.git,
        node: value.node,
        powershell: value.powershell,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-powershell-mismatch/,
    );
    assert.equal(calls, 1);
    writeFileSync(value.powershell.path, "prior-powershell");

    writeFileSync(value.git.path, "changed-git");
    await assert.rejects(
      deliverPackageArtifactHandoff({
        callback: async () => {
          calls += 1;
        },
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        receiptBytes,
        rawRoot: "retained-raw-root",
        frozen: { contract: "frozen" },
        sourceRoot: value.sourceRoot,
        sourceManifest: value.sourceManifest,
        git: value.git,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-git-mismatch/,
    );
    assert.equal(calls, 1);

    writeFileSync(value.git.path, "staged-git");
    writeFileSync(path.join(value.sourceRoot, "marker.txt"), "changed-source");
    await assert.rejects(
      deliverPackageArtifactHandoff({
        callback: async () => {
          calls += 1;
        },
        scratchRoot: value.scratchRoot,
        artifacts: value.artifacts,
        receipt: value.receipt,
        receiptBytes,
        rawRoot: "retained-raw-root",
        frozen: { contract: "frozen" },
        sourceRoot: value.sourceRoot,
        sourceManifest: value.sourceManifest,
        git: value.git,
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      /artifact-handoff-source-mismatch/,
    );
    assert.equal(calls, 1);
  } finally {
    rmSync(value.scratchRoot, { recursive: true, force: true });
    rmSync(originalRoot, { recursive: true, force: true });
  }
});

test("artifact delivery binds manifest, Git, and Node to the validated receipt", async () => {
  const value = fixture();
  let callbacks = 0;
  const deliver = () =>
    deliverPackageArtifactHandoff({
      callback: async () => {
        callbacks += 1;
      },
      scratchRoot: value.scratchRoot,
      artifacts: value.artifacts,
      receipt: value.receipt,
      receiptBytes: Buffer.from("passing-receipt"),
      rawRoot: "retained-raw-root",
      frozen: { contract: "frozen" },
      sourceRoot: value.sourceRoot,
      sourceManifest: value.sourceManifest,
      git: value.git,
      node: value.node,
      powershell: value.powershell,
      maximumExecutableBytes: 1024,
      maximumInstallerBytes: 1024,
    });
  try {
    const manifestSha256 = value.receipt.source.manifestSha256;
    value.receipt.source.manifestSha256 = "0".repeat(64);
    await assert.rejects(deliver(), /artifact-handoff-source-mismatch/);
    value.receipt.source.manifestSha256 = manifestSha256;

    const gitSha256 = value.receipt.buildIdentity.git.executableSha256;
    value.receipt.buildIdentity.git.executableSha256 = "0".repeat(64);
    await assert.rejects(deliver(), /artifact-handoff-git-receipt-mismatch/);
    value.receipt.buildIdentity.git.executableSha256 = gitSha256;

    value.receipt.buildIdentity.node.executableSha256 = "0".repeat(64);
    await assert.rejects(deliver(), /artifact-handoff-node-receipt-mismatch/);
    assert.equal(callbacks, 0);
  } finally {
    rmSync(value.scratchRoot, { recursive: true, force: true });
  }
});

test("runner mutation inside the authenticated handoff callback fails before launch", async () => {
  const value = fixture();
  try {
    for (const relativePath of INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS) {
      const destination = path.join(
        value.sourceRoot,
        ...relativePath.split("/"),
      );
      mkdirSync(path.dirname(destination), { recursive: true });
      cpSync(path.resolve(relativePath), destination);
    }
    value.sourceManifest = buildSourceManifest(value.sourceRoot, {
      files: ["marker.txt", ...INSTALLED_SMOKE_RUNNER_RELATIVE_PATHS],
      directories: [],
    });
    value.receipt.source = {
      aggregateSha256: value.sourceManifest.aggregateSha256,
      manifestSha256: digest(JSON.stringify(value.sourceManifest)).sha256,
      fileCount: value.sourceManifest.fileCount,
      bytes: value.sourceManifest.totalBytes,
    };
    let childCalls = 0;
    await deliverPackageArtifactHandoff({
      callback: async (payload) => {
        const stagedRunner = path.join(
          payload.sourceRoot,
          "scripts",
          "measure-windows-installed-smoke.ps1",
        );
        writeFileSync(stagedRunner, "mutated after handoff authentication");
        await assert.rejects(
          runAuthenticatedInstalledSmoke({
            handoff: payload,
            campaignId: "handoff-callback-mutation",
            outputDirectory: path.join(value.scratchRoot, "smoke-output"),
            expectedSignerThumbprint: "f".repeat(40),
            runProcess: async () => {
              childCalls += 1;
            },
          }),
          /installed-smoke-runner-identity-changed/,
        );
        cpSync(
          path.resolve("scripts/measure-windows-installed-smoke.ps1"),
          stagedRunner,
        );
        writeFileSync(payload.tools.powershell.path, "mutated after handoff");
        await assert.rejects(
          runAuthenticatedInstalledSmoke({
            handoff: payload,
            campaignId: "handoff-powershell-mutation",
            outputDirectory: path.join(
              value.scratchRoot,
              "powershell-smoke-output",
            ),
            expectedSignerThumbprint: "f".repeat(40),
            runProcess: async () => {
              childCalls += 1;
            },
          }),
          /installed-smoke-powershell-identity-changed/,
        );
      },
      scratchRoot: value.scratchRoot,
      artifacts: value.artifacts,
      receipt: value.receipt,
      receiptBytes: Buffer.from("passing-receipt"),
      rawRoot: "retained-raw-root",
      frozen: { contract: "frozen" },
      sourceRoot: value.sourceRoot,
      sourceManifest: value.sourceManifest,
      git: value.git,
      node: value.node,
      powershell: value.powershell,
      maximumExecutableBytes: 1024 * 1024,
      maximumInstallerBytes: 1024,
    });
    assert.equal(childCalls, 0);
  } finally {
    rmSync(value.scratchRoot, { recursive: true, force: true });
  }
});
