import { describe, expect, it } from "vitest";

import type {
  BootstrapSnapshot,
  ResourceRef,
  TrashProjectResourceResult,
} from "../../domain/workbench";
import { validateResourceTrashResult } from "./resourceTrashContract";

const source = { rootId: "root-b", relativePath: "src/old" };
const parent = { rootId: "root-b", relativePath: "src" };

function snapshot(selectedResource: ResourceRef | null = parent) {
  return {
    transitionEpoch: 6,
    project: { id: "project-a", roots: [{ id: "root-b" }] },
    selectedResource,
    selectedRootId: "root-b",
    tabs: [],
  } as unknown as BootstrapSnapshot;
}

function committed(): TrashProjectResourceResult {
  return {
    outcome: "committed",
    resourceConfirmed: true,
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 6,
    source,
    parent,
    parentListing: { resource: parent, entries: [], truncated: false },
    snapshot: snapshot(),
    warning: null,
  };
}

const expected = {
  projectId: "project-a",
  projectGeneration: 3,
  transitionEpoch: 5,
  source,
};

describe("resource trash contract", () => {
  it("accepts exact confirmed absence and typed unchanged unsupported", () => {
    expect(validateResourceTrashResult(committed(), expected)).toEqual({
      outcome: "committed",
      resourceConfirmed: true,
    });
    expect(
      validateResourceTrashResult(
        {
          ...committed(),
          outcome: "unsupported",
          resourceConfirmed: false,
          transitionEpoch: 5,
          parentListing: null,
          snapshot: null,
          warning: "Operating-system trash is unavailable.",
        },
        expected,
      ),
    ).toEqual({ outcome: "unsupported", resourceConfirmed: false });
  });

  it("rejects descendant tabs, source-present listings, stale epochs, and malformed unsupported", () => {
    const invalid: TrashProjectResourceResult[] = [
      { ...committed(), transitionEpoch: 5 },
      {
        ...committed(),
        parentListing: {
          resource: parent,
          truncated: false,
          entries: [
            { name: "old", resource: source, kind: "directory", byteLen: null },
          ],
        },
      },
      {
        ...committed(),
        snapshot: {
          ...snapshot(),
          tabs: [
            {
              id: "descendant",
              kind: "diff",
              title: "child.ts",
              position: 0,
              pinned: false,
              dirty: false,
              resource: { rootId: "root-b", relativePath: "src/old/child.ts" },
            },
          ],
        },
      },
      {
        ...committed(),
        outcome: "unsupported",
        resourceConfirmed: false,
        transitionEpoch: 5,
        warning: "Unsupported",
      },
    ];
    for (const result of invalid) {
      expect(() => validateResourceTrashResult(result, expected)).toThrow(
        /stale or invalid/,
      );
    }
  });
});
