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

import { sha256 } from "./ade0006a-c0b-common.mjs";
import { stagePackageSource } from "./ade0006a-package-source-stage.mjs";

function manifest(files) {
  const records = Object.entries(files)
    .map(([relativePath, value]) => ({
      path: relativePath,
      bytes: Buffer.byteLength(value),
      sha256: sha256(value),
    }))
    .sort((left, right) =>
      Buffer.compare(Buffer.from(left.path), Buffer.from(right.path)),
    );
  return {
    fileCount: records.length,
    totalBytes: records.reduce((total, file) => total + file.bytes, 0),
    files: records,
    include: {
      bounds: {
        files: 16,
        sourceBytes: 1024,
        fileBytes: 512,
        directoryDepth: 8,
      },
    },
  };
}

test("source staging copies only the exact admitted manifest projection", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-stage-"));
  try {
    const source = path.join(root, "source");
    const staged = path.join(root, "staged");
    mkdirSync(path.join(source, "src"), { recursive: true });
    writeFileSync(path.join(source, "package.json"), "package");
    writeFileSync(path.join(source, "src", "main.ts"), "source");
    writeFileSync(path.join(source, "ignored.txt"), "not-admitted");
    const sourceManifest = manifest({
      "package.json": "package",
      "src/main.ts": "source",
    });
    const result = await stagePackageSource({
      sourceRoot: source,
      sourceManifest,
      destinationRoot: staged,
    });
    assert.deepEqual(result, {
      sourceRoot: staged,
      fileCount: 2,
      bytes: 13,
    });
    assert.equal(
      readFileSync(path.join(staged, "src", "main.ts"), "utf8"),
      "source",
    );
    assert.throws(
      () => readFileSync(path.join(staged, "ignored.txt")),
      /ENOENT/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("source staging rejects drift and removes its incomplete projection", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-stage-"));
  try {
    const source = path.join(root, "source");
    const staged = path.join(root, "staged");
    mkdirSync(source);
    writeFileSync(path.join(source, "package.json"), "changed");
    await assert.rejects(
      stagePackageSource({
        sourceRoot: source,
        sourceManifest: manifest({ "package.json": "expected" }),
        destinationRoot: staged,
      }),
      /source manifest identity changed/,
    );
    assert.throws(
      () => readFileSync(path.join(staged, "package.json")),
      /ENOENT/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
