import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  COMPLETE_MARKER,
  IGNORED_DIRECTORIES,
  MAX_SYNTHETIC_FILES,
  NEEDLE_EVERY,
  SYNTHETIC_NEEDLE,
  measureSyntheticTree,
  parseArguments,
  syntheticTree,
  syntheticTreeManifest,
  writeSyntheticTree,
} from "./synthetic-tree.mjs";

test("the same seed makes the same tree and another seed a different one", () => {
  const first = syntheticTreeManifest({ seed: 7, count: 5_000 });
  assert.deepEqual(syntheticTreeManifest({ seed: 7, count: 5_000 }), first);
  assert.notEqual(
    syntheticTreeManifest({ seed: 8, count: 5_000 }).sha256,
    first.sha256,
  );
  assert.equal(first.files, 5_000);
});

test("a fixed seed keeps its exact tree across releases", () => {
  // Pinned so later tickets compare timings on an identical tree. Changing
  // the generator must change this hash on purpose, never by accident.
  assert.equal(
    syntheticTreeManifest({ seed: 1, count: 2_000 }).sha256,
    "f31fd41d39030415511fac291dc4febf908ca5124ddebde11e0410eefdf0637d",
  );
});

test("paths are unique, relative and shaped like a real workspace", () => {
  const paths = [...syntheticTree({ seed: 3, count: 20_000 })].map(
    (file) => file.path,
  );
  assert.equal(new Set(paths).size, paths.length);
  for (const relative of paths) {
    assert.ok(
      !relative.startsWith("/") &&
        !relative.includes("\\") &&
        !relative.includes(".."),
    );
  }
  const manifest = syntheticTreeManifest({ seed: 3, count: 20_000 });
  assert.ok(manifest.directories > 500, `directories ${manifest.directories}`);
  assert.ok(
    manifest.maxDepth >= 5 && manifest.maxDepth <= 7,
    `depth ${manifest.maxDepth}`,
  );
  // About one file in ten sits under an ignored folder, split evenly.
  assert.ok(
    Math.abs(manifest.ignoredFiles / 20_000 - 0.1) < 0.02,
    `ignored ${manifest.ignoredFiles}`,
  );
  for (const [root, files] of Object.entries(manifest.ignoredByRoot)) {
    assert.ok(Math.abs(files / manifest.ignoredFiles - 1 / 3) < 0.1, root);
  }
  assert.equal(manifest.visibleFiles + manifest.ignoredFiles, 20_000);
  assert.equal(manifest.needleFiles, Math.floor((20_000 - 1) / NEEDLE_EVERY));
  assert.ok(manifest.visibleNeedleFiles < manifest.needleFiles);
});

test("paths stay valid on case-insensitive and Windows file systems", () => {
  const reserved = /^(?:con|prn|aux|nul|com\d|lpt\d)(?:\.|$)/iu;
  const seen = new Set();
  const directories = new Set();
  for (const { path: relative, content } of syntheticTree({
    seed: 2,
    count: 50_000,
  })) {
    // APFS and NTFS are case-insensitive by default.
    const folded = relative.toLowerCase();
    assert.ok(!seen.has(folded), `case collision ${relative}`);
    seen.add(folded);
    const parts = folded.split("/");
    for (let end = 1; end < parts.length; end += 1) {
      directories.add(parts.slice(0, end).join("/"));
    }
    for (const part of parts) {
      assert.ok(!reserved.test(part), `reserved name ${relative}`);
      assert.ok(!/[<>:"|?*\\]/u.test(part), `invalid character ${relative}`);
    }
    // Leaves 140 characters of MAX_PATH for the tree's own root.
    assert.ok(relative.length <= 120, `long path ${relative}`);
    if (content.includes(SYNTHETIC_NEEDLE)) assert.match(relative, /\.ts$/u);
  }
  for (const directory of directories) {
    assert.ok(!seen.has(directory), `file and folder share ${directory}`);
  }
});

test("the .gitignore names every ignored directory", () => {
  const [first] = syntheticTree({ seed: 1, count: 1 });
  assert.equal(first.path, ".gitignore");
  assert.deepEqual(
    first.content.trim().split("\n"),
    IGNORED_DIRECTORIES.map((name) => `${name}/`),
  );
});

test("writes into an empty directory and refuses a non-empty one", () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-synthetic-tree-"));
  try {
    const target = path.join(root, "tree");
    const manifest = writeSyntheticTree(target, { seed: 5, count: 1_500 });
    // The returned manifest is measured from disk, so it proves the write.
    assert.deepEqual(
      manifest,
      syntheticTreeManifest({ seed: 5, count: 1_500 }),
    );
    assert.deepEqual(
      JSON.parse(readFileSync(path.join(target, COMPLETE_MARKER), "utf8")),
      { seed: 5, count: 1_500, ...manifest },
    );
    const lost = [...syntheticTree({ seed: 5, count: 1_500 })].at(-1);
    rmSync(path.join(target, ...lost.path.split("/")));
    assert.notEqual(measureSyntheticTree(target).sha256, manifest.sha256);
    assert.equal(measureSyntheticTree(target).files, 1_499);
    const needle = [...syntheticTree({ seed: 5, count: 1_500 })].find((file) =>
      file.content.includes(SYNTHETIC_NEEDLE),
    );
    assert.equal(
      readFileSync(path.join(target, ...needle.path.split("/")), "utf8"),
      needle.content,
    );

    const busy = path.join(root, "busy");
    writeSyntheticTree(busy, { seed: 5, count: 1 });
    writeFileSync(path.join(busy, "keep.txt"), "mine");
    assert.throws(
      () => writeSyntheticTree(busy, { seed: 5, count: 1 }),
      /not empty/u,
    );
    assert.equal(readFileSync(path.join(busy, "keep.txt"), "utf8"), "mine");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects counts and seeds outside the bounded range", () => {
  assert.throws(() => syntheticTreeManifest({ count: 0 }), /count/u);
  assert.throws(
    () => syntheticTreeManifest({ count: MAX_SYNTHETIC_FILES + 1 }),
    /count/u,
  );
  assert.throws(() => syntheticTreeManifest({ seed: -1 }), /seed/u);
  assert.throws(() => syntheticTreeManifest({ seed: 1.5 }), /seed/u);
});

test("the command line rejects missing or malformed values", () => {
  assert.deepEqual(parseArguments(["--seed", "4", "--count", "10"]), {
    seed: 4,
    count: 10,
    out: null,
  });
  assert.equal(parseArguments(["--out", "tree"]).out, "tree");
  assert.throws(
    () => parseArguments(["--count", "5", "--out"]),
    /needs a value/u,
  );
  assert.throws(
    () => parseArguments(["--out", "--count", "5"]),
    /needs a value/u,
  );
  assert.throws(() => parseArguments(["--seed", " "]), /needs a value/u);
  assert.throws(() => parseArguments(["--seed", "1e3"]), /whole number/u);
  assert.throws(() => parseArguments(["--size", "1"]), /unknown argument/u);
});
