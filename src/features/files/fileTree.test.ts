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
    ).toEqual(["src", "README.md"]);
    expect(
      flattenFileTree(root, listings, {
        [resourceKey(srcRef)]: true,
      }).entries.map((entry) => entry.resource.relativePath),
    ).toEqual(["src", "src/main.ts", "README.md"]);
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
