import assert from "node:assert/strict";
import {
  existsSync,
  linkSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { sha256 } from "./ade0006a-c0b-common.mjs";
import { snapshotPackageArtifact } from "./ade0006a-package-snapshot.mjs";

test("artifact snapshot admits one stable file and owns immutable bytes", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-snapshot-"));
  try {
    const source = path.join(root, "source.exe");
    const destination = path.join(root, "snapshot.exe");
    const bytes = Buffer.from("authenticated-package-artifact");
    writeFileSync(source, bytes);
    const observed = await snapshotPackageArtifact({
      sourcePath: source,
      destinationPath: destination,
      maximumBytes: 1024,
    });
    assert.deepEqual(observed, {
      path: destination,
      sha256: sha256(bytes),
      bytes: bytes.byteLength,
    });
    writeFileSync(source, "source-changed-after-admission");
    assert.deepEqual(readFileSync(destination), bytes);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("artifact snapshot rejects linked sources and never leaves a partial target", async () => {
  for (const kind of ["symbolic", "hard"]) {
    const root = mkdtempSync(path.join(tmpdir(), "sage-package-snapshot-"));
    try {
      const source = path.join(root, "source.exe");
      const linked = path.join(root, "linked.exe");
      const destination = path.join(root, "snapshot.exe");
      writeFileSync(source, "package-artifact");
      if (kind === "symbolic") symlinkSync(source, linked);
      else linkSync(source, linked);
      await assert.rejects(
        snapshotPackageArtifact({
          sourcePath: kind === "symbolic" ? linked : source,
          destinationPath: destination,
          maximumBytes: 1024,
        }),
        /regular non-linked file/,
      );
      assert.equal(existsSync(destination), false);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});
