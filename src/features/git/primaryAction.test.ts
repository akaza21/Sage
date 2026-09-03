import { describe, expect, it } from "vitest";

import type { RootRepositoryStatus } from "../../domain/workbench";
import { repositoryPrimaryAction } from "./primaryAction";

function repository(
  overrides: Partial<RootRepositoryStatus> = {},
): RootRepositoryStatus {
  return {
    rootId: "root-a",
    state: "repository",
    branch: "main",
    changes: [],
    truncated: false,
    elapsedMs: 1,
    message: null,
    remotes: ["origin"],
    ...overrides,
  };
}

describe("repositoryPrimaryAction", () => {
  it("uses only native-proven state and never guesses from a branch name", () => {
    expect(repositoryPrimaryAction(repository())).toEqual({
      kind: "publish",
      label: "Publish Branch",
      remote: "origin",
      reason: null,
    });
    expect(
      repositoryPrimaryAction(
        repository({ upstream: "origin/main", ahead: 2 }),
      ),
    ).toEqual({ kind: "push", label: "Push", remote: "origin", reason: null });
    expect(
      repositoryPrimaryAction(
        repository({ upstream: "origin/main", behind: 1 }),
      ),
    ).toEqual({ kind: "pull", label: "Pull", reason: null });
    expect(repositoryPrimaryAction(repository({ remotes: [] }))).toBeNull();
  });

  it("only offers Pull for a clean branch that is behind without divergence", () => {
    expect(
      repositoryPrimaryAction(
        repository({ upstream: "origin/main", ahead: 1, behind: 1 }),
      ),
    ).toEqual({
      kind: "sync",
      label: "Sync Changes",
      reason:
        "This branch has diverged from its upstream. Sage cannot yet choose a merge or rebase safely.",
    });
    expect(
      repositoryPrimaryAction(
        repository({
          upstream: "origin/main",
          behind: 1,
          changes: [
            {
              resource: { rootId: "root-a", relativePath: "dirty.txt" },
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 1,
              deletions: 0,
            },
          ],
        }),
      ),
    ).toMatchObject({ kind: "sync" });
    expect(
      repositoryPrimaryAction(
        repository({ upstream: "origin/main", behind: 1, truncated: true }),
      ),
    ).toMatchObject({ kind: "sync" });
  });

  it("prioritizes a real commit over transport and rejects detached or unavailable repositories", () => {
    expect(
      repositoryPrimaryAction(
        repository({
          changes: [
            {
              resource: { rootId: "root-a", relativePath: "a.ts" },
              indexStatus: "modified",
              worktreeStatus: "unchanged",
              additions: 1,
              deletions: 0,
            },
          ],
        }),
      ),
    ).toMatchObject({ kind: "commit" });
    expect(
      repositoryPrimaryAction(repository({ branch: "HEAD (detached)" })),
    ).toBeNull();
    expect(
      repositoryPrimaryAction(repository({ state: "unavailable" })),
    ).toBeNull();
  });

  it("hides transport actions during every recovery sequence or conflict", () => {
    for (const sequence of [
      "mergeInProgress",
      "rebaseInProgress",
      "cherryPickInProgress",
      "revertInProgress",
    ] as const) {
      const unpublished = repositoryPrimaryAction(
        repository({ [sequence]: true }),
      );
      const ahead = repositoryPrimaryAction(
        repository({
          [sequence]: true,
          upstream: "origin/main",
          ahead: 1,
        }),
      );
      expect(["publish", "push", "pull", "sync"]).not.toContain(
        unpublished?.kind,
      );
      expect(["publish", "push", "pull", "sync"]).not.toContain(ahead?.kind);
    }
    expect(
      repositoryPrimaryAction(
        repository({
          changes: [
            {
              resource: { rootId: "root-a", relativePath: "conflict.ts" },
              indexStatus: "conflicted",
              worktreeStatus: "conflicted",
              additions: null,
              deletions: null,
            },
          ],
        }),
      ),
    ).toBeNull();
  });
});
