import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import {
  observeCandidateCharacterization,
  observeProductionBuild,
  resolvePackageBuildTools,
} from "./ade0006a-package-build.mjs";

const frozen = loadFrozenContracts(path.resolve("."));

function fingerprint(features) {
  const root = mkdtempSync(path.join(tmpdir(), "sage-cargo-fingerprint-"));
  const directory = path.join(
    root,
    "release",
    ".fingerprint",
    "sage-0123456789abcdef",
  );
  mkdirSync(directory, { recursive: true });
  writeFileSync(
    path.join(directory, "bin-sage.json"),
    JSON.stringify({ features: JSON.stringify(features) }),
  );
  return root;
}

test("production build evidence comes from the exact Cargo fingerprint", async () => {
  const root = fingerprint(["custom-protocol", "default"]);
  try {
    const observed = await observeProductionBuild({
      participant: "candidate",
      targetDirectory: root,
      frozen,
    });
    assert.deepEqual(observed.enabledFeatures, ["custom-protocol"]);
    assert.equal(observed.defaultFeaturesEnabled, true);
    assert.match(observed.fingerprintSha256, /^[a-f0-9]{64}$/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("production build evidence rejects a missing contracted feature", async () => {
  const root = fingerprint(["default"]);
  try {
    await assert.rejects(
      observeProductionBuild({
        participant: "candidate",
        targetDirectory: root,
        frozen,
      }),
      /cargo-production-fingerprint-features-mismatch/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("candidate characterization binds the exact feature fingerprint and executable", async () => {
  const root = fingerprint(["acp-fixture", "custom-protocol", "default"]);
  writeFileSync(path.join(root, "release", "sage.exe"), "characterization");
  try {
    const observed = await observeCandidateCharacterization({
      targetDirectory: root,
      frozen,
    });
    assert.deepEqual(observed.summary.featureSet, [
      "acp-fixture",
      "custom-protocol",
    ]);
    assert.equal(observed.summary.defaultFeaturesEnabled, true);
    assert.equal(observed.summary.executableBytes, 16);
    assert.match(observed.summary.fingerprintSha256, /^[a-f0-9]{64}$/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("package tools derive distinct Cargo and Rustc binaries from the pinned toolchain root", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-tools-"));
  try {
    const rustToolchainRoot = path.join(
      root,
      frozen.package.authenticatedBuild.rust.toolchainDirectoryName,
    );
    const cargo = path.join(rustToolchainRoot, "bin", "cargo.exe");
    const rustc = path.join(rustToolchainRoot, "bin", "rustc.exe");
    const node = path.join(root, "node");
    const git = path.join(root, "git");
    const tauriJs = path.join(
      root,
      "node_modules",
      "@tauri-apps",
      "cli",
      "tauri.js",
    );
    const tauriNative = path.join(
      root,
      "node_modules",
      "@tauri-apps",
      "cli-win32-x64-msvc",
      "cli.win32-x64-msvc.node",
    );
    mkdirSync(path.dirname(cargo), { recursive: true });
    mkdirSync(path.dirname(tauriJs), { recursive: true });
    mkdirSync(path.dirname(tauriNative), { recursive: true });
    for (const file of [cargo, rustc, node, git, tauriJs, tauriNative]) {
      writeFileSync(file, file);
    }
    const tools = await resolvePackageBuildTools(
      {
        sourceRoot: root,
        rustToolchainRoot,
        node,
        git,
      },
      frozen,
    );
    assert.equal(tools.cargo.path, cargo);
    assert.equal(tools.rustc.path, rustc);
    assert.notEqual(tools.cargo.sha256, tools.rustc.sha256);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
