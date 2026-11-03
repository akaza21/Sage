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
  it.each([
    {
      upstream: "team/fork/main",
      remotes: ["team/fork"],
      branch: "main",
      remote: "team/fork",
    },
    {
      upstream: "team/fork/feature/x",
      remotes: ["team/fork"],
      branch: "feature/x",
      remote: "team/fork",
    },
    {
      upstream: "team/fork/main",
      remotes: ["team"],
      branch: "fork/main",
      remote: "team",
    },
    {
      upstream: "origin/main",
      remotes: ["origin"],
      branch: "main",
      remote: "origin",
    },
    {
      upstream: "origin/feature/x",
      remotes: ["origin"],
      branch: "feature/x",
      remote: "origin",
    },
  ])(
    "offers Push to $remote for matching branch $branch on $upstream",
    ({ upstream, remotes, branch, remote }) => {
      expect(
        repositoryPrimaryAction(
          repository({ upstream, remotes, branch, ahead: 1 }),
        ),
      ).toEqual({ kind: "push", label: "Push", remote, reason: null });
    },
  );

  it.each([
    { remotes: ["team", "team/fork"], branch: "main" },
    { remotes: ["team/fork", "team"], branch: "main" },
    { remotes: ["team", "team/fork"], branch: "fork/main" },
    { remotes: ["team/fork", "team"], branch: "fork/main" },
  ])(
    "hides Push for ambiguous remotes $remotes even when branch $branch matches one",
    ({ remotes, branch }) => {
      expect(
        repositoryPrimaryAction(
          repository({ upstream: "team/fork/main", remotes, branch, ahead: 1 }),
        ),
      ).toBeNull();
    },
  );

  it.each([
    { upstream: "team/fork/main", remotes: ["team/fork"], branch: "topic" },
    { upstream: "origin/main", remotes: ["origin"], branch: "topic" },
    { upstream: "team/fork/feature/x", remotes: ["team/fork"], branch: "x" },
    { upstream: "team/fork/main", remotes: ["team"], branch: "main" },
  ])(
    "hides Push when local branch $branch differs from $upstream",
    ({ upstream, remotes, branch }) => {
      expect(
        repositoryPrimaryAction(
          repository({ upstream, remotes, branch, ahead: 1 }),
        ),
      ).toBeNull();
    },
  );

  it.each([
    { upstream: "unlisted/main", remotes: ["origin"] },
    { upstream: "team/fork/main", remotes: ["origin"] },
    { upstream: "team/forked/main", remotes: ["team/fork"] },
    { upstream: "main", remotes: ["main"] },
    { upstream: "origin/", remotes: ["origin"] },
    { upstream: "team/fork/", remotes: ["team/fork"] },
    { upstream: "origin/main", remotes: [] },
  ])(
    "hides Push without a listed remote and non-empty branch: $upstream with $remotes",
    ({ upstream, remotes }) => {
      expect(
        repositoryPrimaryAction(repository({ upstream, remotes, ahead: 1 })),
      ).toBeNull();
    },
  );

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
