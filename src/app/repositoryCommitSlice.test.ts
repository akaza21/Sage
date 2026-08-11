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
});
