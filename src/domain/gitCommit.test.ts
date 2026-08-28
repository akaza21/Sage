import { describe, expect, it } from "vitest";

import {
  canAbortGitSequence,
  canCommitRepository,
  canCompleteMerge,
  canContinueCherryPick,
  canContinueRebase,
  canContinueRevert,
  exclusiveGitSequence,
  hasStagedChanges,
  MAX_GIT_COMMIT_MESSAGE_BYTES,
  normalizeGitCommitMessage,
} from "./gitCommit";
import type { RootRepositoryStatus } from "./workbench";

function root(
  indexStatus: RootRepositoryStatus["changes"][number]["indexStatus"],
  worktreeStatus: RootRepositoryStatus["changes"][number]["worktreeStatus"] = "unchanged",
): RootRepositoryStatus {
  return {
    rootId: "root-a",
    state: "repository",
    branch: "main",
    changes: [
      {
        resource: { rootId: "root-a", relativePath: "file.txt" },
        indexStatus,
        worktreeStatus,
        additions: 1,
        deletions: 0,
      },
    ],
    truncated: false,
    elapsedMs: 1,
    message: null,
  };
}

describe("Git commit eligibility", () => {
  it("normalizes a bounded UTF-8 message and rejects invalid input", () => {
    expect(normalizeGitCommitMessage("  subject\nbody  ")).toBe(
      "subject\nbody",
    );
    expect(
      normalizeGitCommitMessage("x".repeat(MAX_GIT_COMMIT_MESSAGE_BYTES)),
    ).toHaveLength(MAX_GIT_COMMIT_MESSAGE_BYTES);
    expect(() => normalizeGitCommitMessage(" ")).toThrow(/required/);
    expect(() => normalizeGitCommitMessage("nul\0byte")).toThrow(/null/);
    expect(() =>
      normalizeGitCommitMessage("é".repeat(MAX_GIT_COMMIT_MESSAGE_BYTES)),
    ).toThrow(/UTF-8/);
  });

  it("requires complete, conflict-free status with a known staged change", () => {
    expect(hasStagedChanges(root("modified"))).toBe(true);
    expect(hasStagedChanges(root("unchanged", "modified"))).toBe(false);
    expect(hasStagedChanges(root("unknown"))).toBe(false);
    expect(hasStagedChanges(root("conflicted", "conflicted"))).toBe(false);
    expect(hasStagedChanges({ ...root("modified"), truncated: true })).toBe(
      false,
    );
    expect(
      hasStagedChanges({ ...root("modified"), state: "notRepository" }),
    ).toBe(false);
  });

  it("enables a resolved merge commit even when the index matches HEAD", () => {
    const emptyMerge: RootRepositoryStatus = {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [],
      truncated: false,
      elapsedMs: 1,
      message: null,
      mergeInProgress: true,
    };
    expect(hasStagedChanges(emptyMerge)).toBe(false);
    expect(canCompleteMerge(emptyMerge)).toBe(true);
    expect(canCommitRepository(emptyMerge)).toBe(true);
    expect(
      canCompleteMerge({
        ...emptyMerge,
        changes: [
          {
            resource: { rootId: "root-a", relativePath: "conflict.txt" },
            indexStatus: "conflicted",
            worktreeStatus: "conflicted",
            additions: null,
            deletions: null,
          },
        ],
      }),
    ).toBe(false);
    expect(canCompleteMerge({ ...emptyMerge, truncated: true })).toBe(false);
    expect(canCompleteMerge({ ...emptyMerge, mergeInProgress: false })).toBe(
      false,
    );
    expect(canCompleteMerge({ ...emptyMerge, rebaseInProgress: true })).toBe(
      false,
    );
    expect(canCommitRepository(root("modified"))).toBe(true);
  });

  it("enables continue rebase only after a resolved exclusive rebase", () => {
    const emptyRebase: RootRepositoryStatus = {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [],
      truncated: false,
      elapsedMs: 1,
      message: null,
      rebaseInProgress: true,
    };
    expect(canContinueRebase(emptyRebase)).toBe(true);
    expect(canCompleteMerge(emptyRebase)).toBe(false);
    expect(canCommitRepository(emptyRebase)).toBe(false);
    expect(
      canContinueRebase({
        ...emptyRebase,
        changes: [
          {
            resource: { rootId: "root-a", relativePath: "conflict.txt" },
            indexStatus: "conflicted",
            worktreeStatus: "conflicted",
            additions: null,
            deletions: null,
          },
        ],
      }),
    ).toBe(false);
    expect(canContinueRebase({ ...emptyRebase, truncated: true })).toBe(false);
    expect(canContinueRebase({ ...emptyRebase, mergeInProgress: true })).toBe(
      false,
    );
    expect(canContinueRebase({ ...emptyRebase, rebaseInProgress: false })).toBe(
      false,
    );
    expect(
      canCommitRepository({ ...root("modified"), rebaseInProgress: true }),
    ).toBe(false);
  });

  it("enables continue cherry-pick only after a resolved exclusive cherry-pick", () => {
    const emptyCherryPick: RootRepositoryStatus = {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [],
      truncated: false,
      elapsedMs: 1,
      message: null,
      cherryPickInProgress: true,
    };
    expect(canContinueCherryPick(emptyCherryPick)).toBe(true);
    expect(canCompleteMerge(emptyCherryPick)).toBe(false);
    expect(canContinueRebase(emptyCherryPick)).toBe(false);
    expect(canCommitRepository(emptyCherryPick)).toBe(false);
    expect(
      canContinueCherryPick({
        ...emptyCherryPick,
        changes: [
          {
            resource: { rootId: "root-a", relativePath: "conflict.txt" },
            indexStatus: "conflicted",
            worktreeStatus: "conflicted",
            additions: null,
            deletions: null,
          },
        ],
      }),
    ).toBe(false);
    expect(canContinueCherryPick({ ...emptyCherryPick, truncated: true })).toBe(
      false,
    );
    expect(
      canContinueCherryPick({ ...emptyCherryPick, mergeInProgress: true }),
    ).toBe(false);
    expect(
      canContinueCherryPick({ ...emptyCherryPick, rebaseInProgress: true }),
    ).toBe(false);
    expect(
      canContinueCherryPick({
        ...emptyCherryPick,
        cherryPickInProgress: false,
      }),
    ).toBe(false);
    expect(
      canCommitRepository({
        ...root("modified"),
        cherryPickInProgress: true,
      }),
    ).toBe(false);
    expect(
      canCompleteMerge({
        ...emptyCherryPick,
        mergeInProgress: true,
      }),
    ).toBe(false);
  });

  it("enables continue revert only after a resolved exclusive revert", () => {
    const emptyRevert: RootRepositoryStatus = {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [],
      truncated: false,
      elapsedMs: 1,
      message: null,
      revertInProgress: true,
    };
    expect(canContinueRevert(emptyRevert)).toBe(true);
    expect(canCompleteMerge(emptyRevert)).toBe(false);
    expect(canContinueRebase(emptyRevert)).toBe(false);
    expect(canContinueCherryPick(emptyRevert)).toBe(false);
    expect(canCommitRepository(emptyRevert)).toBe(false);
    expect(
      canContinueRevert({
        ...emptyRevert,
        changes: [
          {
            resource: { rootId: "root-a", relativePath: "conflict.txt" },
            indexStatus: "conflicted",
            worktreeStatus: "conflicted",
            additions: null,
            deletions: null,
          },
        ],
      }),
    ).toBe(false);
    expect(canContinueRevert({ ...emptyRevert, truncated: true })).toBe(false);
    expect(canContinueRevert({ ...emptyRevert, mergeInProgress: true })).toBe(
      false,
    );
    expect(canContinueRevert({ ...emptyRevert, rebaseInProgress: true })).toBe(
      false,
    );
    expect(
      canContinueRevert({ ...emptyRevert, cherryPickInProgress: true }),
    ).toBe(false);
    expect(canContinueRevert({ ...emptyRevert, revertInProgress: false })).toBe(
      false,
    );
    expect(
      canCommitRepository({ ...root("modified"), revertInProgress: true }),
    ).toBe(false);
  });

  it("admits abort for an exclusive in-progress sequence even with conflicts", () => {
    const conflictedMerge: RootRepositoryStatus = {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [
        {
          resource: { rootId: "root-a", relativePath: "conflict.txt" },
          indexStatus: "conflicted",
          worktreeStatus: "conflicted",
          additions: null,
          deletions: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
      message: null,
      mergeInProgress: true,
    };
    expect(exclusiveGitSequence(conflictedMerge)).toBe("merge");
    expect(canAbortGitSequence(conflictedMerge)).toBe(true);
    expect(
      canAbortGitSequence({
        ...conflictedMerge,
        mergeInProgress: false,
        rebaseInProgress: true,
      }),
    ).toBe(true);
    expect(
      canAbortGitSequence({
        ...conflictedMerge,
        rebaseInProgress: true,
      }),
    ).toBe(false);
    expect(canAbortGitSequence({ ...conflictedMerge, truncated: true })).toBe(
      false,
    );
    expect(
      canAbortGitSequence({ ...conflictedMerge, mergeInProgress: false }),
    ).toBe(false);
  });
});
