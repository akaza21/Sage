import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { prepareIsolatedPackageBuildHome } from "./ade0006a-package-build-home.mjs";

test("isolated package home keeps its linker temp directory durable", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-home-"));
  const cargoHome = path.join(root, "cargo-home");
  mkdirSync(cargoHome);
  try {
    const home = await prepareIsolatedPackageBuildHome({
      root: path.join(root, "build"),
      cargoHome,
    });
    assert.equal(
      readFileSync(
        path.join(home.profileRoot, "Temp", ".sage-build-home"),
        "utf8",
      ),
      "1",
    );
    assert.equal(readFileSync(home.npmUserConfig, "utf8"), "");
    assert.equal(statSync(home.viteCache).isDirectory(), true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("isolated package home rejects Cargo configuration overrides", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-home-"));
  const cargoHome = path.join(root, "cargo-home");
  mkdirSync(cargoHome);
  writeFileSync(path.join(cargoHome, "config.toml"), "[build]");
  try {
    await assert.rejects(
      prepareIsolatedPackageBuildHome({
        root: path.join(root, "build"),
        cargoHome,
      }),
      /cargo-home-configuration-forbidden/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
