import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import { loadFrozenContracts } from "./ade0006a-c0b-contracts.mjs";
import { deriveAuthenticatedPackageBuildIdentity } from "./ade0006a-package-build-identity.mjs";
import { validateAuthenticatedPackageBuildIdentity } from "./ade0006a-package-build-identity-validation.mjs";

const frozen = loadFrozenContracts(path.resolve("."));
const hash = (value) => value.repeat(64).slice(0, 64);

function tree(records = []) {
  return {
    summary: { treeSha256: hash("a"), fileCount: 1, bytes: 1 },
    records,
  };
}

function identity() {
  const contract = frozen.package.authenticatedBuild;
  return deriveAuthenticatedPackageBuildIdentity({
    contract,
    versions: {
      node: contract.node.version,
      npm: contract.node.npmVersion,
      cargo: {
        release: contract.rust.release,
        commit: contract.rust.cargoCommit,
      },
      rustc: {
        release: contract.rust.release,
        host: contract.rust.host,
        commit: contract.rust.rustcCommit,
      },
      git: contract.git.version,
    },
    buildTools: {
      cargo: { sha256: hash("b") },
      rustc: { sha256: hash("c") },
      "tauri-cli-js": { sha256: hash("d") },
      "tauri-cli-native": { sha256: hash("e") },
    },
    node: { sha256: hash("f") },
    powershell: { sha256: hash("d") },
    npmCli: { sha256: hash("1") },
    git: { sha256: hash("2") },
    launchers: {
      typescript: { sha256: hash("3") },
      vite: { sha256: hash("4") },
    },
    msvc: {
      cl: { sha256: hash("5") },
      link: { sha256: hash("6") },
    },
    sdk: {
      rc: { sha256: hash("7") },
      mt: { sha256: hash("8") },
    },
    trees: {
      rust: tree(),
      cargoRegistry: tree(),
      npm: tree(),
      nodeModules: tree(),
      gitRuntime: tree(),
      msvcInputs: tree(),
      windowsSdkInputs: tree(),
      wix: tree([
        { type: "regularFile", relativePath: "candle.exe", sha256: hash("9") },
        { type: "regularFile", relativePath: "light.exe", sha256: hash("0") },
        { type: "regularFile", relativePath: "dark.exe", sha256: hash("a") },
      ]),
      nsis: tree([
        {
          type: "regularFile",
          relativePath: "makensis.exe",
          sha256: hash("b"),
        },
        {
          type: "regularFile",
          relativePath: "Bin/makensis.exe",
          sha256: hash("c"),
        },
      ]),
    },
    windowsSdkVersion: "10.0.26100.0",
  });
}

test("authenticated build identity rejects Rustup-proxy-style Cargo/Rustc aliasing", () => {
  const observed = identity();
  const accepted = [];
  validateAuthenticatedPackageBuildIdentity(
    accepted,
    observed.buildIdentity,
    frozen,
  );
  assert.deepEqual(accepted, []);
  observed.buildIdentity.rustToolchain.rustcExecutableSha256 =
    observed.buildIdentity.rustToolchain.cargoExecutableSha256;
  const rejected = [];
  validateAuthenticatedPackageBuildIdentity(
    rejected,
    observed.buildIdentity,
    frozen,
  );
  assert.ok(rejected.some((error) => error.includes("pinned real toolchain")));
});

test("authenticated build identity binds the prior PowerShell executable", () => {
  const observed = identity();
  assert.equal(observed.buildIdentity.powershell.executableSha256, hash("d"));
  const accepted = [];
  validateAuthenticatedPackageBuildIdentity(
    accepted,
    observed.buildIdentity,
    frozen,
  );
  assert.deepEqual(accepted, []);
  delete observed.buildIdentity.powershell;
  const rejected = [];
  validateAuthenticatedPackageBuildIdentity(
    rejected,
    observed.buildIdentity,
    frozen,
  );
  assert.ok(rejected.some((error) => error.includes("PowerShell")));
});
