import { describe, expect, it } from "vitest";

import type {
  FileChange,
  ProjectRepositoryStatus,
  RootRepositoryStatus,
} from "../domain/repository";
import { shareRepositoryStatus } from "./shareRepositoryStatus";

const change = (relativePath: string): FileChange => ({
  resource: { rootId: "a", relativePath },
  indexStatus: "unchanged",
  worktreeStatus: "modified",
  additions: 1,
  deletions: null,
});

const root = (
  rootId: string,
  changes: FileChange[],
  elapsedMs = 5,
): RootRepositoryStatus => ({
  rootId,
  state: "repository",
  branch: "main",
  ahead: 0,
  changes,
  truncated: false,
  elapsedMs,
  message: null,
});

const project = (
  roots: RootRepositoryStatus[],
  elapsedMs = 5,
): ProjectRepositoryStatus => ({ roots, truncated: false, elapsedMs });

describe("shareRepositoryStatus", () => {
  it("returns the previous status when only timings differ", () => {
    const previous = project([root("a", [change("x.ts")])]);
    const next = project([root("a", [change("x.ts")], 40)], 41);
    expect(shareRepositoryStatus(previous, next)).toBe(previous);
  });

  it("keeps unchanged roots and replaces changed ones", () => {
    const previous = project([
      root("a", [change("x.ts")]),
      root("b", [change("y.ts")]),
    ]);
    const next = project([
      root("a", [change("x.ts")], 9),
      root("b", [change("y.ts"), change("z.ts")]),
    ]);
    const shared = shareRepositoryStatus(previous, next);
    expect(shared).not.toBe(previous);
    expect(shared.roots[0]).toBe(previous.roots[0]);
    expect(shared.roots[1]).toBe(next.roots[1]);
  });

  it("sees a changed field, a missing field and a reordered root", () => {
    const previous = project([root("a", []), root("b", [])]);
    const behind = project([{ ...root("a", []), behind: 1 }, root("b", [])]);
    expect(shareRepositoryStatus(previous, behind)).not.toBe(previous);
    const { ahead: _ahead, ...withoutAhead } = root("a", []);
    const missing = project([withoutAhead, root("b", [])]);
    expect(shareRepositoryStatus(previous, missing)).not.toBe(previous);
    const reordered = project([root("b", []), root("a", [])]);
    expect(shareRepositoryStatus(previous, reordered)).not.toBe(previous);
    const unstaged = project([root("a", [change("x.ts")]), root("b", [])]);
    const staged = project([
      root("a", [{ ...change("x.ts"), indexStatus: "modified" }]),
      root("b", []),
    ]);
    expect(shareRepositoryStatus(unstaged, staged).roots[0]).toBe(
      staged.roots[0],
    );
  });
});
