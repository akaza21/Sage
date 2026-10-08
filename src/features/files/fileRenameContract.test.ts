import { describe, expect, it } from "vitest";

import type {
  BootstrapSnapshot,
  RenameProjectFileResult,
  ResourceRef,
} from "../../domain/workbench";
import {
  renameTargetResource,
  validateFileRenameResult,
} from "./fileRenameContract";

const source = { rootId: "root-a", relativePath: "src/old.ts" };
const target = { rootId: "root-a", relativePath: "src/new.ts" };
const parent = { rootId: "root-a", relativePath: "src" };

function snapshot(selectedResource: ResourceRef | null = target) {
  return {
    transitionEpoch: 6,
    project: { id: "project-a", roots: [{ id: "root-a" }] },
    selectedResource,
    selectedRootId: "root-a",
    tabs: [],
  } as unknown as BootstrapSnapshot;
}

function result(): RenameProjectFileResult {
  return {
    outcome: "committed",
    resourceConfirmed: true,
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 6,
    source,
    target,
    parentListing: {
      resource: parent,
      truncated: false,
      entries: [{ name: "new.ts", resource: target, kind: "file", byteLen: 9 }],
    },
    snapshot: snapshot(),
    warning: null,
  };
}

describe("file rename contract", () => {
  it("derives one same-parent target and accepts exact authoritative identity", () => {
    expect(renameTargetResource(source, "new.ts")).toEqual(target);
    expect(
      validateFileRenameResult(result(), {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 5,
        source,
        target,
      }),
    ).toBe(true);
  });

  it("keeps a confirmed rename committed when only durability is unconfirmed", () => {
    expect(
      validateFileRenameResult(
        {
          ...result(),
          warning:
            "File was renamed, but its durability could not be confirmed.",
        },
        {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 5,
          source,
          target,
        },
      ),
    ).toBe(true);
  });

  it("rejects stale, source-present, truncated, and unconfirmed committed results", () => {
    const cases: RenameProjectFileResult[] = [
      { ...result(), transitionEpoch: 5 },
      { ...result(), transitionEpoch: 7 },
      { ...result(), resourceConfirmed: false },
      {
        ...result(),
        parentListing: {
          ...result().parentListing!,
          entries: [
            ...result().parentListing!.entries,
            { name: "old.ts", resource: source, kind: "file", byteLen: 9 },
          ],
        },
      },
      {
        ...result(),
        parentListing: { ...result().parentListing!, truncated: true },
      },
      { ...result(), snapshot: snapshot(source) },
    ];
    for (const value of cases) {
      expect(() =>
        validateFileRenameResult(value, {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 5,
          source,
          target,
        }),
      ).toThrow("native file rename returned stale or invalid identity");
    }
  });

  it("allows a bounded effect-unknown result without a confirmed listing", () => {
    const value = {
      ...result(),
      outcome: "effectUnknown" as const,
      resourceConfirmed: false,
      parentListing: null,
      snapshot: snapshot(source),
      warning: "Rename outcome is unknown; Explorer was refreshed.",
    };
    expect(
      validateFileRenameResult(value, {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 5,
        source,
        target,
      }),
    ).toBe(false);
  });

  it("allows a typed effect-unknown envelope when snapshot recovery fails", () => {
    expect(
      validateFileRenameResult(
        {
          ...result(),
          outcome: "effectUnknown",
          snapshot: null,
          warning: "The authoritative layout snapshot was unavailable.",
        },
        {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 5,
          source,
          target,
        },
      ),
    ).toBe(true);
  });
});
