import { describe, expect, it } from "vitest";

import {
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
});
