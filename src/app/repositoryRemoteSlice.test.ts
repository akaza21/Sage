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
      remotes: ["origin"],
      remotesTruncated: false,
      changes: [],
      truncated: false,
      elapsedMs: 1,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 1,
};

const afterRepositories: ProjectRepositoryStatus = {
  ...beforeRepositories,
  elapsedMs: 2,
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
      ],
    },
    repositories: beforeRepositories,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(useWorkbenchStore.getInitialState(), true);
});

describe("repositoryRemoteSlice", () => {
  it.each(["project", "command ownership"])(
    "drops a late fetch result after %s changes and preserves the new state",
    async (changed) => {
      const pending =
        deferred<Awaited<ReturnType<typeof desktop.fetchGitRemote>>>();
      vi.spyOn(desktop, "fetchGitRemote").mockReturnValue(pending.promise);
      const action = useWorkbenchStore
        .getState()
        .fetchRemote("root-a", "origin");
      expect(useWorkbenchStore.getState().repositoryRemote).toEqual({
        rootId: "root-a",
        remote: "origin",
        action: "fetch",
      });
      useWorkbenchStore.setState({
        ...(changed === "project"
          ? {
              project: {
                ...useWorkbenchStore.getState().project!,
                id: "project-b",
              },
              projectGeneration: 4,
            }
          : {
              repositoryRemote: {
                rootId: "root-a",
                remote: "origin",
                action: "push" as const,
              },
            }),
        repositoryRemoteError: {
          rootId: "root-a",
          remote: "origin",
          action: "push",
          kind: "failed",
          message: "Keep the new remote error",
        },
      });
      const replacement = useWorkbenchStore.getState();

      pending.resolve({
        projectId: "project-a",
        rootId: "root-a",
        transitionEpoch: 7,
        repositories: afterRepositories,
        error: null,
      });

      await expect(action).resolves.toBe(false);
      expect(useWorkbenchStore.getState()).toEqual({
        ...replacement,
        repositoryRemote:
          changed === "project" ? null : replacement.repositoryRemote,
      });
    },
  );

  it("surfaces a typed native fetch failure and clears busy", async () => {
    vi.spyOn(desktop, "fetchGitRemote").mockRejectedValue({
      kind: "containment",
      message: "Git fetch may still be running",
      processState: "potentiallyRunning",
    });

    await expect(
      useWorkbenchStore.getState().fetchRemote("root-a", "origin"),
    ).resolves.toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      repositories: beforeRepositories,
      repositoryRemote: null,
      repositoryRemoteError: {
        rootId: "root-a",
        remote: "origin",
        action: "fetch",
        kind: "containment",
        message: "Git fetch may still be running",
        processState: "potentiallyRunning",
      },
    });
  });

  it("fetches a listed remote name and replaces Source Control status", async () => {
    const fetchRemote = vi.spyOn(desktop, "fetchGitRemote").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });

    await expect(
      useWorkbenchStore.getState().fetchRemote("root-a", "origin"),
    ).resolves.toBe(true);

    expect(fetchRemote).toHaveBeenCalledWith(
      { projectId: "project-a", rootId: "root-a", remote: "origin" },
      7,
    );
    expect(useWorkbenchStore.getState().repositories).toEqual(
      afterRepositories,
    );
    expect(useWorkbenchStore.getState().repositoryRemote).toBeNull();
    expect(useWorkbenchStore.getState().repositoryRemoteError).toBeNull();
  });

  it("pushes the current named branch to a listed remote name", async () => {
    const pushRemote = vi.spyOn(desktop, "pushGitRemote").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: afterRepositories,
      error: null,
    });

    await expect(
      useWorkbenchStore.getState().pushRemote("root-a", "origin"),
    ).resolves.toBe(true);

    expect(pushRemote).toHaveBeenCalledWith(
      { projectId: "project-a", rootId: "root-a", remote: "origin" },
      7,
    );
  });

  it("pulls only the selected repository and replaces Source Control status", async () => {
    const pullRepository = vi
      .spyOn(desktop, "pullGitRepository")
      .mockResolvedValue({
        projectId: "project-a",
        rootId: "root-a",
        transitionEpoch: 7,
        repositories: afterRepositories,
        error: null,
      });

    await expect(
      useWorkbenchStore.getState().pullRepository("root-a"),
    ).resolves.toBe(true);

    expect(pullRepository).toHaveBeenCalledWith(
      { projectId: "project-a", rootId: "root-a" },
      7,
    );
    expect(useWorkbenchStore.getState().repositories).toEqual(
      afterRepositories,
    );
  });

  it("does not fetch while a commit is in flight", async () => {
    const fetchRemote = vi.spyOn(desktop, "fetchGitRemote");
    useWorkbenchStore.setState({ repositoryCommit: { rootId: "root-a" } });

    await expect(
      useWorkbenchStore.getState().fetchRemote("root-a", "origin"),
    ).resolves.toBe(false);
    expect(fetchRemote).not.toHaveBeenCalled();
  });
});
