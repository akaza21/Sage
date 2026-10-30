import { describe, expect, it } from "vitest";

import { resourceKey } from "../../domain/resourceIdentity";
import type { DirectoryListing, ResourceRef } from "../../domain/workbench";
import { flattenFileTree } from "./fileTree";

const root: ResourceRef = { rootId: "root-a", relativePath: "" };
const srcRef: ResourceRef = { rootId: "root-a", relativePath: "src" };
const rootListing: DirectoryListing = {
  resource: root,
  truncated: false,
  entries: [
    { name: "src", resource: srcRef, kind: "directory", byteLen: null },
    {
      name: "README.md",
      resource: { rootId: "root-a", relativePath: "README.md" },
      kind: "file",
      byteLen: 100,
    },
    {
      name: "debug.log",
      resource: { rootId: "root-a", relativePath: "debug.log" },
      kind: "file",
      byteLen: 10,
      ignored: true,
    },
  ],
};
const srcListing: DirectoryListing = {
  resource: srcRef,
  truncated: false,
  entries: [
    {
      name: "main.ts",
      resource: { rootId: "root-a", relativePath: "src/main.ts" },
      kind: "file",
      byteLen: 80,
    },
  ],
};

describe("file tree projection", () => {
  it("only projects children of expanded directories", () => {
    const listings = {
      [resourceKey(root)]: { phase: "ready" as const, value: rootListing },
      [resourceKey(srcRef)]: { phase: "ready" as const, value: srcListing },
    };
    expect(
      flattenFileTree(root, listings, {}).entries.map(
        (entry) => entry.resource.relativePath,
      ),
    ).toEqual(["src", "README.md", "debug.log"]);
    expect(
      flattenFileTree(root, listings, {
        [resourceKey(srcRef)]: true,
      }).entries.map((entry) => entry.resource.relativePath),
    ).toEqual(["src", "src/main.ts", "README.md", "debug.log"]);
  });

  it("leaves out ignored entries and their children when they are hidden", () => {
    const ignoredSrc: DirectoryListing = {
      ...rootListing,
      entries: rootListing.entries.map((entry) =>
        entry.name === "src" ? { ...entry, ignored: true } : entry,
      ),
    };
    const listings = {
      [resourceKey(root)]: { phase: "ready" as const, value: ignoredSrc },
      [resourceKey(srcRef)]: { phase: "ready" as const, value: srcListing },
    };
    const expanded = { [resourceKey(srcRef)]: true };
    expect(
      flattenFileTree(root, listings, expanded, 600, true).entries.map(
        (entry) => entry.resource.relativePath,
      ),
    ).toEqual(["README.md"]);
    expect(
      flattenFileTree(root, listings, expanded).entries.map(
        (entry) => entry.resource.relativePath,
      ),
    ).toEqual(["src", "src/main.ts", "README.md", "debug.log"]);
  });

  it("enforces the visible-node limit", () => {
    const result = flattenFileTree(
      root,
      {
        [resourceKey(root)]: { phase: "ready", value: rootListing },
      },
      {},
      1,
    );
    expect(result.entries).toHaveLength(1);
    expect(result.truncated).toBe(true);
  });
});
