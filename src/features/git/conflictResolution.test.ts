import { describe, expect, it } from "vitest";

import type { FileChange } from "../../domain/workbench";
import { canResolveConflictedChange } from "./conflictResolution";

function change(
  path: string,
  indexStatus: FileChange["indexStatus"],
  worktreeStatus: FileChange["worktreeStatus"] = "unchanged",
): FileChange {
  return {
    resource: { rootId: "root-a", relativePath: path },
    indexStatus,
    worktreeStatus,
    additions: null,
    deletions: null,
  };
}

describe("canResolveConflictedChange", () => {
  it("admits Keep ours and Keep theirs only for conflicted files", () => {
    const conflicted = change("bad.ts", "conflicted", "conflicted");
    expect(canResolveConflictedChange(conflicted, "keepOurs")).toBe(true);
    expect(canResolveConflictedChange(conflicted, "keepTheirs")).toBe(true);
    expect(canResolveConflictedChange(conflicted, "stage")).toBe(false);
    expect(
      canResolveConflictedChange(change("ok.ts", "modified"), "keepOurs"),
    ).toBe(false);
  });
});
