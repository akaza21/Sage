import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  observeAuthenticatedPackageTreeSet,
  verifyAuthenticatedPackageTreeSet,
} from "./ade0006a-package-tree-set.mjs";

const bounds = { members: 8, bytes: 128, pathUtf8Bytes: 64 };

test("authenticated package tree set binds every exposed input subtree", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-tree-set-"));
  const first = path.join(root, "first");
  const second = path.join(root, "second");
  mkdirSync(first);
  mkdirSync(second);
  writeFileSync(path.join(first, "header.h"), "header");
  writeFileSync(path.join(second, "library.lib"), "library");
  try {
    const observed = await observeAuthenticatedPackageTreeSet({
      members: [
        { label: "headers", root: first },
        { label: "libraries", root: second },
      ],
      bounds,
    });
    assert.equal(observed.summary.fileCount, 2);
    assert.equal(observed.summary.bytes, 13);
    await verifyAuthenticatedPackageTreeSet(observed, bounds);
    writeFileSync(path.join(second, "library.lib"), "changed");
    await assert.rejects(
      verifyAuthenticatedPackageTreeSet(observed, bounds),
      /tree set changed/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("authenticated package tree set enforces aggregate bounds", async () => {
  const root = mkdtempSync(path.join(tmpdir(), "sage-package-tree-set-"));
  const first = path.join(root, "first");
  const second = path.join(root, "second");
  mkdirSync(first);
  mkdirSync(second);
  writeFileSync(path.join(first, "a"), "1");
  writeFileSync(path.join(second, "b"), "2");
  try {
    await assert.rejects(
      observeAuthenticatedPackageTreeSet({
        members: [
          { label: "first", root: first },
          { label: "second", root: second },
        ],
        bounds: { ...bounds, members: 1 },
      }),
      /exceeds its bounds/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
