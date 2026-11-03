import {
  validateCreateProjectFileResult,
  validateCreateProjectFolderResult,
} from "./newFileResult";
import { describe, expect, it } from "vitest";

import { resourceKey } from "../../domain/resourceIdentity";
import type { ProjectSummary, WorkbenchTab } from "../../domain/workbench";
import {
  explorerCreateParent,
  MAX_NEW_FILE_NAME_BYTES,
  validateNewFileBasename,
  validateNewFilePath,
} from "./newFileContract";

const project: ProjectSummary = {
  id: "project-a",
  name: "A",
  roots: [
    {
      id: "root-a",
      name: "A",
      ordinal: 0,
      availability: "available",
      message: null,
    },
    {
      id: "root-b",
      name: "B",
      ordinal: 1,
      availability: "available",
      message: null,
    },
  ],
};

describe("new file portable contract", () => {
  it("accepts exactly 255 UTF-8 bytes and rejects plus one", () => {
    expect(
      validateNewFileBasename("a".repeat(MAX_NEW_FILE_NAME_BYTES)),
    ).toBeNull();
    expect(
      validateNewFileBasename("a".repeat(MAX_NEW_FILE_NAME_BYTES + 1)),
    ).toContain("UTF-8");
    expect(validateNewFileBasename(`${"é".repeat(127)}a`)).toBeNull();
    expect(validateNewFileBasename("é".repeat(128))).toContain("UTF-8");
  });

  it.each([
    "..",
    "a/b",
    "a\\b",
    "CON",
    "nul.txt",
    "COM¹",
    "com².ts",
    "CoM³",
    "LPT¹",
    "lpt².md",
    "LpT³",
    "bad.",
    "bad ",
    "a\u0000b",
    "a\u007fb",
    "a*b",
  ])("rejects non-portable basename %j", (basename) =>
    expect(validateNewFileBasename(basename)).not.toBeNull(),
  );

  it("bounds the full root-relative UTF-8 path", () => {
    expect(
      validateNewFilePath(
        { rootId: "root-a", relativePath: "a".repeat(16_382) },
        "x",
      ),
    ).toBeNull();
    expect(
      validateNewFilePath(
        { rootId: "root-a", relativePath: "a".repeat(16_383) },
        "x",
      ),
    ).toContain("path exceeds");
  });

  it("accepts a committed warning projection but rejects missing committed disk truth", () => {
    const parent = { rootId: "root-a", relativePath: "src" };
    const resource = { rootId: "root-a", relativePath: "src/new.ts" };
    const result = {
      outcome: "committed" as const,
      resourceConfirmed: true,
      projectId: "project-a",
      projectGeneration: 3,
      transitionEpoch: 4,
      resource,
      parentListing: {
        resource: parent,
        entries: [
          { name: "new.ts", resource, kind: "file" as const, byteLen: 0 },
        ],
        truncated: false,
      },
      warning: "Created, but durability sync failed.",
    };
    expect(() =>
      validateCreateProjectFileResult(result, {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 4,
        parent,
        resource,
      }),
    ).not.toThrow();
    expect(() =>
      validateCreateProjectFileResult(
        { ...result, parentListing: { ...result.parentListing, entries: [] } },
        {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 4,
          parent,
          resource,
        },
      ),
    ).toThrow("invalid identity");

    expect(() =>
      validateCreateProjectFileResult(
        {
          ...result,
          outcome: "effectUnknown",
          resourceConfirmed: false,
          parentListing: { ...result.parentListing, entries: [] },
        },
        {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 4,
          parent,
          resource,
        },
      ),
    ).not.toThrow();
  });

  it("validates a folder only when the authoritative listing carries directory identity", () => {
    const parent = { rootId: "root-b", relativePath: "src" };
    const resource = { rootId: "root-b", relativePath: "src/nested" };
    const result = {
      outcome: "committed" as const,
      resourceConfirmed: true,
      projectId: "project-a",
      projectGeneration: 3,
      transitionEpoch: 4,
      resource,
      parentListing: {
        resource: parent,
        entries: [
          {
            name: "nested",
            resource,
            kind: "directory" as const,
            byteLen: null,
          },
        ],
        truncated: false,
      },
      warning: null,
    };
    expect(() =>
      validateCreateProjectFolderResult(result, {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 4,
        parent,
        resource,
      }),
    ).not.toThrow();
    expect(() =>
      validateCreateProjectFolderResult(
        {
          ...result,
          parentListing: {
            ...result.parentListing,
            entries: [
              {
                ...result.parentListing.entries[0],
                kind: "file" as const,
                byteLen: 0,
              },
            ],
          },
        },
        {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 4,
          parent,
          resource,
        },
      ),
    ).toThrow("invalid identity");
  });
});

describe("Explorer create parent", () => {
  const file = { rootId: "root-a", relativePath: "src/main.ts" };
  const tab: WorkbenchTab = {
    id: "file-a",
    kind: "file",
    title: "main.ts",
    position: 0,
    pinned: false,
    dirty: true,
    resource: file,
  };

  it("uses an active file's explicitly previewed parent", () => {
    expect(
      explorerCreateParent(project, "root-a", file, [tab], tab.id, {}),
    ).toEqual({ rootId: "root-a", relativePath: "src" });
  });

  it("keeps same relative directories in different roots distinct", () => {
    const directory = { rootId: "root-b", relativePath: "src" };
    expect(
      explorerCreateParent(project, "root-b", directory, [], null, {
        [resourceKey(directory)]: { phase: "loading" },
      }),
    ).toEqual(directory);
  });
});
