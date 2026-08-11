import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
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
  const artifacts = {};
  const receipt = { result: "passed", participants: {} };
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
          assert.equal(
            readFileSync(path.join(payload.sourceRoot, "marker.txt"), "utf8"),
            "staged-source",
          );
          assert.equal(payload.tools.git.sha256, value.git.sha256);
          assert.equal(payload.tools.git.bytes, value.git.bytes);
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
        maximumExecutableBytes: 1024,
        maximumInstallerBytes: 1024,
      }),
      true,
    );
    assert.equal(calls, 1);
    assert.equal(completed, true);
    assert.equal(value.receipt.result, "passed");
    assert.equal(receiptBytes.toString(), "passing-receipt");

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
