import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ProjectRepositoryStatus } from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const beforeRepositories: ProjectRepositoryStatus = {
  roots: [
    {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      changes: [
        {
          resource: { rootId: "root-a", relativePath: "same.txt" },
          indexStatus: "modified",
          worktreeStatus: "modified",
          additions: 1,
          deletions: 1,
        },
      ],
      truncated: false,
      elapsedMs: 1,
      message: null,
    },
    {
      rootId: "root-b",
      state: "repository",
      branch: "main",
      changes: [
        {
          resource: { rootId: "root-b", relativePath: "same.txt" },
          indexStatus: "modified",
          worktreeStatus: "unchanged",
          additions: 2,
          deletions: 0,
        },
      ],
      truncated: false,
      elapsedMs: 1,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 2,
};

const afterRepositories: ProjectRepositoryStatus = {
  ...beforeRepositories,
  roots: [
    { ...beforeRepositories.roots[0], changes: [] },
    beforeRepositories.roots[1],
  ],
};

beforeEach(() => {
  useWorkbenchStore.setState(useWorkbenchStore.getInitialState(), true);
  useWorkbenchStore.setState({
    phase: "ready",
    projectGeneration: 3,
    nativeTransitionEpoch: 7,
    projectTransitioning: false,
    repositoryRefreshing: false,
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "Alpha",
          ordinal: 0,
          availability: "available",
          message: null,
        },
        {
          id: "root-b",
          name: "Beta",
          ordinal: 1,
          availability: "available",
          message: null,
        },
      ],
    },
    repositories: beforeRepositories,
  });
});

afterEach(() => vi.restoreAllMocks());

describe("repository commit store", () => {
  it("refuses an empty message without calling git", async () => {
    const commit = vi.spyOn(desktop, "commitGitRepository");
    await expect(
      useWorkbenchStore.getState().commitRepository("root-a", "   "),
    ).resolves.toBe(false);
    expect(commit).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositoryCommit: null,
      repositoryCommitError: {
        rootId: "root-a",
        kind: "failed",
        message: "Commit message is required",
      },
    });
  });

  it("refuses to start while another repository command is running", async () => {
    const commit = vi.spyOn(desktop, "commitGitRepository");
    useWorkbenchStore.setState({
      repositoryCheckout: { rootId: "root-b", branch: "main" },
    });
    await expect(
      useWorkbenchStore.getState().commitRepository("root-a", "Busy"),
    ).resolves.toBe(false);
    expect(commit).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().repositoryCommit).toBeNull();
  });

  it("commits one root and adopts its authoritative all-root projection", async () => {
    const commit = vi.spyOn(desktop, "commitGitRepository").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });
    useWorkbenchStore.setState({ error: "Chat error remains scoped to Chat" });

    await expect(
      useWorkbenchStore
        .getState()
        .commitRepository("root-a", "  Root A only  "),
    ).resolves.toBe(true);

    expect(commit).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
        message: "Root A only",
      },
      7,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: afterRepositories,
      repositoryCommit: null,
      repositoryCommitError: null,
      error: "Chat error remains scoped to Chat",
    });
  });

  it("adopts effect-unknown status but preserves prior status for failed and containment results", async () => {
    vi.spyOn(desktop, "commitGitRepository").mockResolvedValueOnce({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: {
        kind: "effectUnknown",
        message: "Git commit outcome is uncertain; status was refreshed.",
      },
    });

    await expect(
      useWorkbenchStore.getState().commitRepository("root-a", "uncertain"),
    ).resolves.toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: afterRepositories,
      repositoryCommitError: {
        rootId: "root-a",
        kind: "effectUnknown",
        message: "Git commit outcome is uncertain; status was refreshed.",
      },
    });

    useWorkbenchStore.setState({
      repositories: beforeRepositories,
      repositoryCommitError: null,
    });
    vi.spyOn(desktop, "commitGitRepository").mockRejectedValueOnce({
      kind: "containment",
      processState: "potentiallyRunning",
      message: "Git process may still be running",
    });
    await expect(
      useWorkbenchStore.getState().commitRepository("root-a", "contained"),
    ).resolves.toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: beforeRepositories,
      repositoryCommitError: {
        rootId: "root-a",
        kind: "containment",
        processState: "potentiallyRunning",
      },
    });

    useWorkbenchStore.setState({ repositoryCommitError: null });
    vi.spyOn(desktop, "commitGitRepository").mockRejectedValueOnce(
      new Error("Git identity missing"),
    );
    await expect(
      useWorkbenchStore.getState().commitRepository("root-a", "failed"),
    ).resolves.toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: beforeRepositories,
      repositoryCommitError: {
        rootId: "root-a",
        kind: "failed",
        message: "Git identity missing",
      },
    });
  });

  it("preserves effect-unknown through draft edits and failed refresh, then clears it after a successful full refresh", async () => {
    useWorkbenchStore.setState({
      repositoryCommitError: {
        rootId: "root-a",
        kind: "effectUnknown",
        message: "Commit outcome is uncertain",
      },
    });

    useWorkbenchStore.getState().clearRepositoryCommitError("root-a");
    expect(useWorkbenchStore.getState().repositoryCommitError?.kind).toBe(
      "effectUnknown",
    );

    vi.spyOn(desktop, "getRepositoryStatus").mockRejectedValueOnce(
      new Error("refresh failed"),
    );
    await useWorkbenchStore.getState().refreshRepository();
    expect(useWorkbenchStore.getState().repositoryCommitError?.kind).toBe(
      "effectUnknown",
    );

    vi.spyOn(desktop, "getRepositoryStatus").mockResolvedValueOnce(
      afterRepositories,
    );
    await useWorkbenchStore.getState().refreshRepository();
    expect(useWorkbenchStore.getState().repositoryCommitError).toBeNull();
  });

  it("retires a completion after Project intent changes", async () => {
    const pending =
      deferred<Awaited<ReturnType<typeof desktop.commitGitRepository>>>();
    vi.spyOn(desktop, "commitGitRepository").mockReturnValue(pending.promise);
    const commit = useWorkbenchStore
      .getState()
      .commitRepository("root-a", "stale");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().repositoryCommit).toEqual({
        rootId: "root-a",
      }),
    );
    const nextRepositories: ProjectRepositoryStatus = {
      roots: [],
      truncated: false,
      elapsedMs: 0,
    };
    useWorkbenchStore.setState({
      projectGeneration: 4,
      nativeTransitionEpoch: 8,
      repositories: nextRepositories,
      repositoryCommit: null,
      repositoryCommitError: null,
    });
    pending.resolve({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });

    await expect(commit).resolves.toBe(false);
    expect(useWorkbenchStore.getState().repositories).toBe(nextRepositories);
    expect(useWorkbenchStore.getState().repositoryCommitError).toBeNull();
  });

  it("commits one complete root when an unrelated root truncates the aggregate", async () => {
    const commit = vi.spyOn(desktop, "commitGitRepository").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        truncated: true,
        roots: [
          beforeRepositories.roots[0],
          { ...beforeRepositories.roots[1], truncated: true },
        ],
      },
    });

    await expect(
      useWorkbenchStore
        .getState()
        .commitRepository("root-a", "complete root only"),
    ).resolves.toBe(true);
    expect(commit).toHaveBeenCalledTimes(1);
  });

  it("commits a resolved merge when the index matches HEAD", async () => {
    const commit = vi.spyOn(desktop, "commitGitRepository").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            changes: [],
            mergeInProgress: true,
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().commitRepository("root-a", "Merge other"),
    ).resolves.toBe(true);
    expect(commit).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
        message: "Merge other",
      },
      7,
    );
  });

  it("continues a resolved rebase without inventing a commit message", async () => {
    const continueRebase = vi
      .spyOn(desktop, "continueGitRebase")
      .mockResolvedValue({
        projectId: "project-a",
        rootId: "root-a",
        transitionEpoch: 7,
        repositories: afterRepositories,
        error: null,
      });
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            changes: [],
            rebaseInProgress: true,
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().continueRebase("root-a"),
    ).resolves.toBe(true);
    expect(continueRebase).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
      },
      7,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: afterRepositories,
      repositoryCommit: null,
      repositoryCommitError: null,
    });
  });

  it("refuses continue rebase while conflicts remain", async () => {
    const continueRebase = vi.spyOn(desktop, "continueGitRebase");
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            rebaseInProgress: true,
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "conflict.txt" },
                indexStatus: "conflicted",
                worktreeStatus: "conflicted",
                additions: null,
                deletions: null,
              },
            ],
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().continueRebase("root-a"),
    ).resolves.toBe(false);
    expect(continueRebase).not.toHaveBeenCalled();
  });

  it("continues a resolved cherry-pick without inventing a commit message", async () => {
    const continueCherryPick = vi
      .spyOn(desktop, "continueGitCherryPick")
      .mockResolvedValue({
        projectId: "project-a",
        rootId: "root-a",
        transitionEpoch: 7,
        repositories: afterRepositories,
        error: null,
      });
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            changes: [],
            cherryPickInProgress: true,
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().continueCherryPick("root-a"),
    ).resolves.toBe(true);
    expect(continueCherryPick).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
      },
      7,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: afterRepositories,
      repositoryCommit: null,
      repositoryCommitError: null,
    });
  });

  it("refuses continue cherry-pick while conflicts remain", async () => {
    const continueCherryPick = vi.spyOn(desktop, "continueGitCherryPick");
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            cherryPickInProgress: true,
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "conflict.txt" },
                indexStatus: "conflicted",
                worktreeStatus: "conflicted",
                additions: null,
                deletions: null,
              },
            ],
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().continueCherryPick("root-a"),
    ).resolves.toBe(false);
    expect(continueCherryPick).not.toHaveBeenCalled();
  });

  it("continues a resolved revert without inventing a commit message", async () => {
    const continueRevert = vi
      .spyOn(desktop, "continueGitRevert")
      .mockResolvedValue({
        projectId: "project-a",
        rootId: "root-a",
        transitionEpoch: 7,
        repositories: afterRepositories,
        error: null,
      });
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            changes: [],
            revertInProgress: true,
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().continueRevert("root-a"),
    ).resolves.toBe(true);
    expect(continueRevert).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
      },
      7,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: afterRepositories,
      repositoryCommit: null,
      repositoryCommitError: null,
    });
  });

  it("refuses continue revert while conflicts remain", async () => {
    const continueRevert = vi.spyOn(desktop, "continueGitRevert");
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            revertInProgress: true,
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "conflict.txt" },
                indexStatus: "conflicted",
                worktreeStatus: "conflicted",
                additions: null,
                deletions: null,
              },
            ],
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().continueRevert("root-a"),
    ).resolves.toBe(false);
    expect(continueRevert).not.toHaveBeenCalled();
  });

  it("aborts an exclusive in-progress rebase even while conflicts remain", async () => {
    const abort = vi.spyOn(desktop, "abortGitInProgress").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });
    useWorkbenchStore.setState({
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            rebaseInProgress: true,
            changes: [
              {
                resource: { rootId: "root-a", relativePath: "conflict.txt" },
                indexStatus: "conflicted",
                worktreeStatus: "conflicted",
                additions: null,
                deletions: null,
              },
            ],
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().abortInProgress("root-a"),
    ).resolves.toBe(true);
    expect(abort).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
      },
      7,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: afterRepositories,
      repositoryCommit: null,
      repositoryCommitError: null,
    });
  });

  it("does not abort while continue is in flight", async () => {
    const abort = vi.spyOn(desktop, "abortGitInProgress");
    useWorkbenchStore.setState({
      repositoryCommit: { rootId: "root-a" },
      repositories: {
        ...beforeRepositories,
        roots: [
          {
            ...beforeRepositories.roots[0],
            rebaseInProgress: true,
            changes: [],
          },
          beforeRepositories.roots[1],
        ],
      },
    });

    await expect(
      useWorkbenchStore.getState().abortInProgress("root-a"),
    ).resolves.toBe(false);
    expect(abort).not.toHaveBeenCalled();
  });
});
