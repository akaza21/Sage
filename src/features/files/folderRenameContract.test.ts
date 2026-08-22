import { describe, expect, it } from "vitest";

import type {
  RenameProjectFolderResult,
  ResourceRef,
} from "../../domain/workbench";
import {
  renameFolderTargetResource,
  validateFolderRenameResult,
} from "./folderRenameContract";

const source: ResourceRef = { rootId: "root-a", relativePath: "src/empty" };
const target: ResourceRef = {
  rootId: "root-a",
  relativePath: "src/renamed",
};
const parent: ResourceRef = { rootId: "root-a", relativePath: "src" };

function result(): RenameProjectFolderResult {
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
      entries: [
        {
          name: "renamed",
          resource: target,
          kind: "directory",
          byteLen: null,
        },
      ],
    },
    snapshot: {
      transitionEpoch: 6,
      project: { id: "project-a", roots: [{ id: "root-a" }] },
      tabs: [],
      selectedResource: target,
    } as unknown as RenameProjectFolderResult["snapshot"],
    warning: null,
  };
}

describe("folder rename contract", () => {
  it("derives a sibling target and accepts exact empty-folder evidence", () => {
    expect(renameFolderTargetResource(source, "renamed")).toEqual(target);
    expect(
      validateFolderRenameResult(result(), {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 5,
        source,
        target,
      }),
    ).toBe(true);
  });

  it("rejects file evidence for a folder rename", () => {
    const invalid = result();
    invalid.parentListing!.entries[0] = {
      name: "renamed",
      resource: target,
      kind: "file",
      byteLen: 1,
    };
    expect(() =>
      validateFolderRenameResult(invalid, {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 5,
        source,
        target,
      }),
    ).toThrow("invalid identity");
  });

  it("rejects a snapshot that retains a File or Diff tab beneath the folder", () => {
    const invalid = result();
    invalid.snapshot = {
      ...invalid.snapshot!,
      tabs: [
        {
          id: "file:stale-child",
          kind: "file",
          resource: { rootId: "root-a", relativePath: "src/empty/child.ts" },
        },
      ],
    } as unknown as RenameProjectFolderResult["snapshot"];

    expect(() =>
      validateFolderRenameResult(invalid, {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 5,
        source,
        target,
      }),
    ).toThrow("invalid identity");
  });
});
