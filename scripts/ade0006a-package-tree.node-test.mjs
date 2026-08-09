import assert from "node:assert/strict";
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
  copyAuthenticatedPackageTree,
  observeAuthenticatedPackageTree,
} from "./ade0006a-package-tree.mjs";

const bounds = { members: 16, bytes: 1024, pathUtf8Bytes: 256 };

test("authenticated package tree copies and revalidates the complete identity", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-tree-"));
  try {
    const source = path.join(root, "source");
    const destination = path.join(root, "destination");
    mkdirSync(path.join(source, "bin"), { recursive: true });
    writeFileSync(path.join(source, "bin", "tool.exe"), "tool");
    writeFileSync(path.join(source, "config.json"), "config");
    const tree = await observeAuthenticatedPackageTree({
      root: source,
      bounds,
    });
    const copied = await copyAuthenticatedPackageTree({
      tree,
      destinationRoot: destination,
      bounds,
    });
    assert.deepEqual(copied.summary, tree.summary);
    assert.equal(
      readFileSync(path.join(destination, "bin", "tool.exe"), "utf8"),
      "tool",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("authenticated package tree detects source mutation before copying", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-tree-"));
  try {
    const source = path.join(root, "source");
    const destination = path.join(root, "destination");
    mkdirSync(source);
    writeFileSync(path.join(source, "tool.exe"), "first");
    const tree = await observeAuthenticatedPackageTree({
      root: source,
      bounds,
    });
    writeFileSync(path.join(source, "tool.exe"), "other");
    await assert.rejects(
      copyAuthenticatedPackageTree({
        tree,
        destinationRoot: destination,
        bounds,
      }),
      /tree source changed|identity changed/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
