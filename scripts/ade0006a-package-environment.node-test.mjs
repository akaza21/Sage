import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  createPackageCargoEnvironment,
  createPackageNpmEnvironment,
} from "./ade0006a-package-environment.mjs";

function context() {
  const root = path.resolve("/authenticated");
  return {
    systemRoot: path.join(root, "Windows"),
    comSpec: path.join(root, "Windows", "System32", "cmd.exe"),
    profileRoot: path.join(root, "profile"),
    cargoHome: path.join(root, "cargo-home"),
    npmCache: path.join(root, "npm-cache"),
    viteCache: path.join(root, "vite-cache"),
    npmUserConfig: path.join(root, "npm-user.ini"),
    npmGlobalConfig: path.join(root, "npm-global.ini"),
    cargo: path.join(root, "rust", "bin", "cargo.exe"),
    rustc: path.join(root, "rust", "bin", "rustc.exe"),
    node: path.join(root, "node", "node.exe"),
    git: path.join(root, "git", "git.exe"),
    msvcToolsRoot: path.join(root, "msvc"),
    windowsSdkRoot: path.join(root, "sdk"),
    windowsSdkVersion: "10.0.26100.0",
  };
}

test("package child environments are exact allowlists with no ambient injection", () => {
  process.env.NODE_OPTIONS = "--require poison.js";
  process.env.RUSTFLAGS = "-C link-arg=poison";
  process.env.HTTPS_PROXY = "https://secret.invalid";
  process.env.NAPI_RS_NATIVE_LIBRARY_PATH = "C:\\poison";
  try {
    const value = context();
    const npm = createPackageNpmEnvironment(value);
    const cargo = createPackageCargoEnvironment(
      value,
      path.join(path.resolve("/authenticated"), "target"),
    );
    for (const environment of [npm, cargo]) {
      for (const forbidden of [
        "NODE_OPTIONS",
        "RUSTFLAGS",
        "HTTPS_PROXY",
        "NAPI_RS_NATIVE_LIBRARY_PATH",
      ]) {
        assert.equal(forbidden in environment, false);
      }
    }
    assert.deepEqual(Object.keys(npm).sort(), [
      "APPDATA",
      "CI",
      "ComSpec",
      "HOME",
      "LOCALAPPDATA",
      "NO_PROXY",
      "PATH",
      "PATHEXT",
      "SAGE_VITE_CACHE_DIR",
      "SystemRoot",
      "TEMP",
      "TMP",
      "USERPROFILE",
      "WINDIR",
      "npm_config_cache",
      "npm_config_fund",
      "npm_config_globalconfig",
      "npm_config_ignore_scripts",
      "npm_config_proxy",
      "npm_config_update_notifier",
      "npm_config_userconfig",
    ]);
    assert.equal(cargo.CARGO_NET_OFFLINE, "true");
    assert.equal(npm.PATHEXT, ".COM;.EXE;.BAT;.CMD");
    assert.equal(npm.SAGE_VITE_CACHE_DIR, value.viteCache);
    assert.equal(cargo.RUSTC, value.rustc);
    assert.equal(cargo.CARGO_TARGET_DIR.endsWith("target"), true);
  } finally {
    delete process.env.NODE_OPTIONS;
    delete process.env.RUSTFLAGS;
    delete process.env.HTTPS_PROXY;
    delete process.env.NAPI_RS_NATIVE_LIBRARY_PATH;
  }
});
