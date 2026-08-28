import { describe, expect, it } from "vitest";

import type { FileChange } from "../../domain/workbench";
import {
  conflictGroupLabel,
  partitionSourceChanges,
  sourceChangeStatusChip,
} from "./conflictPresentation";

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

describe("sourceChangeStatusChip", () => {
  it("uses a Devin Conflict chip instead of a bang", () => {
    expect(
      sourceChangeStatusChip(change("app.ts", "conflicted", "conflicted")),
    ).toBe("Conflict");
    expect(sourceChangeStatusChip(change("app.ts", "modified"))).toBe("M");
    expect(
      sourceChangeStatusChip(change("new.ts", "unchanged", "untracked")),
    ).toBe("U");
  });
});

describe("conflictGroupLabel", () => {
  it("names an exclusive rebase, cherry-pick, or revert and stays Conflicts otherwise", () => {
    expect(conflictGroupLabel({ rebaseInProgress: true })).toBe("Rebase");
    expect(conflictGroupLabel({ cherryPickInProgress: true })).toBe(
      "Cherry-pick",
    );
    expect(conflictGroupLabel({ revertInProgress: true })).toBe("Revert");
    expect(
      conflictGroupLabel({
        rebaseInProgress: true,
        cherryPickInProgress: true,
      }),
    ).toBe("Conflicts");
    expect(
      conflictGroupLabel({
        cherryPickInProgress: true,
        revertInProgress: true,
      }),
    ).toBe("Conflicts");
    expect(conflictGroupLabel({})).toBe("Conflicts");
  });
});

describe("partitionSourceChanges", () => {
  it("lifts conflicted files ahead of ordinary changes", () => {
    const modified = change("ok.ts", "modified");
    const conflicted = change("bad.ts", "conflicted", "conflicted");
    expect(partitionSourceChanges([modified, conflicted])).toEqual({
      conflicts: [conflicted],
      others: [modified],
    });
  });
});
