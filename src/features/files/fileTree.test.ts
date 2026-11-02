import { describe, expect, it } from "vitest";

import { pathGlobMatcher } from "../../domain/pathGlobMatcher";
import { resourceKey } from "../../domain/resourceIdentity";
import type { DirectoryListing, ResourceRef } from "../../domain/workbench";
import { EXPLORER_ROW_LIMIT, flattenFileTree } from "./fileTree";

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
      flattenFileTree(
        root,
        listings,
        expanded,
        EXPLORER_ROW_LIMIT,
        true,
      ).entries.map((entry) => entry.resource.relativePath),
    ).toEqual(["README.md"]);
    expect(
      flattenFileTree(root, listings, expanded).entries.map(
        (entry) => entry.resource.relativePath,
      ),
    ).toEqual(["src", "src/main.ts", "README.md", "debug.log"]);
  });

  it("leaves out entries files.exclude names, and everything in an excluded folder", () => {
    const listings = {
      [resourceKey(root)]: { phase: "ready" as const, value: rootListing },
      [resourceKey(srcRef)]: { phase: "ready" as const, value: srcListing },
    };
    const expanded = { [resourceKey(srcRef)]: true };
    const project = (patterns: readonly string[]) =>
      flattenFileTree(
        root,
        listings,
        expanded,
        EXPLORER_ROW_LIMIT,
        false,
        pathGlobMatcher(patterns),
      ).entries.map((entry) => entry.resource.relativePath);
    expect(project(["**/*.log"])).toEqual(["src", "src/main.ts", "README.md"]);
    expect(project(["src"])).toEqual(["README.md", "debug.log"]);
    expect(project(["src/*.ts", "README.md"])).toEqual(["src", "debug.log"]);
    expect(project([])).toEqual([
      "src",
      "src/main.ts",
      "README.md",
      "debug.log",
    ]);
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

it("uses the Explorer cap by default and distinguishes exact capacity from overflow", () => {
  const entries = Array.from({ length: 501 }, (_, index) => ({
    name: `file-${index}.ts`,
    resource: { ...root, relativePath: `file-${index}.ts` },
    kind: "file" as const,
    byteLen: 1,
  }));
  const project = (count: number, limit?: number) =>
    flattenFileTree(
      root,
      {
        [resourceKey(root)]: {
          phase: "ready",
          value: {
            resource: root,
            entries: entries.slice(0, count),
            truncated: false,
          },
        },
      },
      {},
      limit,
    );
  expect(project(501).entries).toHaveLength(500);
  expect(project(501).truncated).toBe(true);
  expect(project(500).truncated).toBe(false);
  expect(project(1, 0)).toEqual({ entries: [], truncated: true });
  expect(project(0, 0)).toEqual({ entries: [], truncated: false });
});
