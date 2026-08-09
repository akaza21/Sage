import assert from "node:assert/strict";
import {
  linkSync,
  mkdtempSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { stageAuthenticatedPackageFileSet } from "./ade0006a-package-file-set.mjs";

const bounds = { members: 8, bytes: 128, pathUtf8Bytes: 64 };

test("authenticated package file set stages only the declared runtime", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-file-set-"));
  const source = path.join(root, "source");
  mkdirSync(source);
  writeFileSync(path.join(source, "tool.exe"), "tool");
  writeFileSync(path.join(source, "runtime.dll"), "runtime");
  writeFileSync(path.join(source, "unrelated.dll"), "unrelated");
  try {
    const staged = await stageAuthenticatedPackageFileSet({
      sourceRoot: source,
      destinationRoot: path.join(root, "staged"),
      relativePaths: ["tool.exe", "runtime.dll"],
      bounds,
    });
    assert.equal(staged.summary.fileCount, 2);
    assert.deepEqual(
      staged.records.map(({ relativePath }) => relativePath),
      ["runtime.dll", "tool.exe"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("authenticated package file set rejects a linked source", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-file-set-"));
  const source = path.join(root, "source");
  mkdirSync(source);
  writeFileSync(path.join(source, "outside.exe"), "tool");
  linkSync(path.join(source, "outside.exe"), path.join(source, "tool.exe"));
  try {
    await assert.rejects(
      stageAuthenticatedPackageFileSet({
        sourceRoot: source,
        destinationRoot: path.join(root, "staged"),
        relativePaths: ["tool.exe"],
        bounds,
      }),
      /one regular non-linked file/,
    );
    const staged = await stageAuthenticatedPackageFileSet({
      sourceRoot: source,
      destinationRoot: path.join(root, "staged-hardlink"),
      relativePaths: ["tool.exe"],
      bounds,
      allowSourceHardLinks: true,
    });
    assert.equal(staged.summary.fileCount, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
