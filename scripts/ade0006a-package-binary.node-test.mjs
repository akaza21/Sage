import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { verifyTauriBundleExecutable } from "./ade0006a-package-binary.mjs";

const SOURCE_MARKER = "__TAURI_BUNDLE_TYPE_VAR_UNK";

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

test("verifier accepts only Tauri's exact MSI bundle-type projection", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-binary-"));
  try {
    const source = Buffer.from(`prefix:${SOURCE_MARKER}:suffix`);
    const installed = Buffer.from("prefix:__TAURI_BUNDLE_TYPE_VAR_MSI:suffix");
    const sourcePath = path.join(root, "source.exe");
    const installedPath = path.join(root, "installed.exe");
    writeFileSync(sourcePath, source);
    writeFileSync(installedPath, installed);

    assert.deepEqual(
      await verifyTauriBundleExecutable({
        sourceExecutable: sourcePath,
        installedExecutable: installedPath,
        installer: "msi",
        maximumBytes: 1024,
      }),
      {
        algorithm: "tauri-bundle-type-patch-v1",
        sourceMarkerOffset: 7,
        sourceMarkerOccurrences: 1,
        targetMarkerOccurrences: 1,
        sourceExecutableSha256: sha256(source),
        expectedInstalledExecutableSha256: sha256(installed),
        installedExecutableSha256: sha256(installed),
        installedExecutableBytes: installed.byteLength,
      },
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("verifier preserves pre-existing target markers when projecting NSIS", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-binary-"));
  try {
    const existing = "__TAURI_BUNDLE_TYPE_VAR_NSS";
    const source = Buffer.from(`${existing}:${SOURCE_MARKER}`);
    const installed = Buffer.from(`${existing}:${existing}`);
    const sourcePath = path.join(root, "source.exe");
    const installedPath = path.join(root, "installed.exe");
    writeFileSync(sourcePath, source);
    writeFileSync(installedPath, installed);

    const result = await verifyTauriBundleExecutable({
      sourceExecutable: sourcePath,
      installedExecutable: installedPath,
      installer: "nsis",
      maximumBytes: 1024,
    });
    assert.equal(result.sourceMarkerOffset, existing.length + 1);
    assert.equal(result.sourceMarkerOccurrences, 1);
    assert.equal(result.targetMarkerOccurrences, 2);
    assert.equal(result.expectedInstalledExecutableSha256, sha256(installed));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("verifier rejects any executable change beyond the bundle marker", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-binary-"));
  try {
    const sourcePath = path.join(root, "source.exe");
    const installedPath = path.join(root, "installed.exe");
    writeFileSync(sourcePath, `prefix:${SOURCE_MARKER}:suffix`);
    writeFileSync(installedPath, "prefix:__TAURI_BUNDLE_TYPE_VAR_MSI:suffiy");
    await assert.rejects(
      verifyTauriBundleExecutable({
        sourceExecutable: sourcePath,
        installedExecutable: installedPath,
        installer: "msi",
        maximumBytes: 1024,
      }),
      /changes beyond Tauri's bundle-type projection/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
