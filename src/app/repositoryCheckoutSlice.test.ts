import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ProjectRepositoryStatus } from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

const beforeRepositories: ProjectRepositoryStatus = {
  roots: [
    {
      rootId: "root-a",
      state: "repository",
      branch: "topic",
      branches: ["main", "topic"],
      branchesTruncated: false,
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
  roots: [{ ...beforeRepositories.roots[0], branch: "main" }],
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

describe("repositoryCheckoutSlice", () => {
  it("switches to a listed local branch and replaces Source Control status", async () => {
    const checkout = vi
      .spyOn(desktop, "checkoutGitLocalBranch")
      .mockResolvedValue({
        projectId: "project-a",
        rootId: "root-a",
        transitionEpoch: 7,
        repositories: afterRepositories,
        error: null,
      });

    await expect(
      useWorkbenchStore.getState().checkoutLocalBranch("root-a", "main"),
    ).resolves.toBe(true);

    expect(checkout).toHaveBeenCalledWith(
      { projectId: "project-a", rootId: "root-a", branch: "main" },
      7,
    );
    expect(useWorkbenchStore.getState().repositories).toEqual(
      afterRepositories,
    );
    expect(useWorkbenchStore.getState().repositoryCheckout).toBeNull();
    expect(useWorkbenchStore.getState().repositoryCheckoutError).toBeNull();
  });

  it("does not checkout while a commit is in flight", async () => {
    const checkout = vi.spyOn(desktop, "checkoutGitLocalBranch");
    useWorkbenchStore.setState({ repositoryCommit: { rootId: "root-a" } });

    await expect(
      useWorkbenchStore.getState().checkoutLocalBranch("root-a", "main"),
    ).resolves.toBe(false);
    expect(checkout).not.toHaveBeenCalled();
  });

  it("creates a local branch without switching and replaces Source Control status", async () => {
    const created: ProjectRepositoryStatus = {
      ...beforeRepositories,
      roots: [
        {
          ...beforeRepositories.roots[0],
          branches: ["feature", "main", "topic"],
        },
      ],
    };
    const create = vi.spyOn(desktop, "createGitLocalBranch").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: created,
      error: null,
    });

    await expect(
      useWorkbenchStore.getState().createLocalBranch("root-a", "feature"),
    ).resolves.toBe(true);

    expect(create).toHaveBeenCalledWith(
      { projectId: "project-a", rootId: "root-a", branch: "feature" },
      7,
    );
    expect(useWorkbenchStore.getState().repositories).toEqual(created);
    expect(useWorkbenchStore.getState().repositoryCheckout).toBeNull();
    expect(useWorkbenchStore.getState().repositoryCheckoutError).toBeNull();
    expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
      "topic",
    );
  });

  it("does not create a branch while checkout is in flight", async () => {
    const create = vi.spyOn(desktop, "createGitLocalBranch");
    useWorkbenchStore.setState({
      repositoryCheckout: { rootId: "root-a", branch: "main" },
    });

    await expect(
      useWorkbenchStore.getState().createLocalBranch("root-a", "feature"),
    ).resolves.toBe(false);
    expect(create).not.toHaveBeenCalled();
  });

  it("deletes a listed non-current local branch and replaces Source Control status", async () => {
    const deleted: ProjectRepositoryStatus = {
      ...beforeRepositories,
      roots: [
        {
          ...beforeRepositories.roots[0],
          branches: ["main", "topic"],
        },
      ],
    };
    const remove = vi.spyOn(desktop, "deleteGitLocalBranch").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: deleted,
      error: null,
    });

    await expect(
      useWorkbenchStore.getState().deleteLocalBranch("root-a", "stale"),
    ).resolves.toBe(true);

    expect(remove).toHaveBeenCalledWith(
      { projectId: "project-a", rootId: "root-a", branch: "stale" },
      7,
    );
    expect(useWorkbenchStore.getState().repositories).toEqual(deleted);
    expect(useWorkbenchStore.getState().repositoryCheckout).toBeNull();
    expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
      "topic",
    );
  });

  it("does not delete a branch while create is in flight", async () => {
    const remove = vi.spyOn(desktop, "deleteGitLocalBranch");
    useWorkbenchStore.setState({
      repositoryCheckout: { rootId: "root-a", branch: "feature" },
    });

    await expect(
      useWorkbenchStore.getState().deleteLocalBranch("root-a", "stale"),
    ).resolves.toBe(false);
    expect(remove).not.toHaveBeenCalled();
  });

  it("renames a listed local branch without switching HEAD commit", async () => {
    const renamed: ProjectRepositoryStatus = {
      ...beforeRepositories,
      roots: [
        {
          ...beforeRepositories.roots[0],
          branches: ["feature", "main"],
        },
      ],
    };
    const rename = vi.spyOn(desktop, "renameGitLocalBranch").mockResolvedValue({
      projectId: "project-a",
      rootId: "root-a",
      transitionEpoch: 7,
      repositories: renamed,
      error: null,
    });

    await expect(
      useWorkbenchStore
        .getState()
        .renameLocalBranch("root-a", "topic", "feature"),
    ).resolves.toBe(true);

    expect(rename).toHaveBeenCalledWith(
      {
        projectId: "project-a",
        rootId: "root-a",
        from: "topic",
        to: "feature",
      },
      7,
    );
    expect(useWorkbenchStore.getState().repositories).toEqual(renamed);
    expect(useWorkbenchStore.getState().repositoryCheckout).toBeNull();
    expect(useWorkbenchStore.getState().repositoryCheckoutError).toBeNull();
    expect(useWorkbenchStore.getState().repositories.roots[0].branch).toBe(
      "topic",
    );
  });

  it("does not rename a branch while delete is in flight", async () => {
    const rename = vi.spyOn(desktop, "renameGitLocalBranch");
    useWorkbenchStore.setState({
      repositoryCheckout: { rootId: "root-a", branch: "stale" },
    });

    await expect(
      useWorkbenchStore
        .getState()
        .renameLocalBranch("root-a", "topic", "feature"),
    ).resolves.toBe(false);
    expect(rename).not.toHaveBeenCalled();
  });
});
