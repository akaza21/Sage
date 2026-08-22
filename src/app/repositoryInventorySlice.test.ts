import { afterEach, describe, expect, it, vi } from "vitest";

import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

describe("repository inventory", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("selects a nested repository through the nested status port", async () => {
    const repository = getWorkbenchPorts().repository;
    const ref = { rootId: "root-a", relativePath: "packages/tool" };
    vi.spyOn(repository, "discoverProjectRepositories").mockResolvedValue({
      projectId: "project-a",
      transitionEpoch: 0,
      refs: [ref],
      truncated: false,
    });
    const status = vi
      .spyOn(repository, "getNestedRepositoryStatus")
      .mockResolvedValue({
        projectId: "project-a",
        transitionEpoch: 0,
        ref,
        status: {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          changes: [],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      });

    useWorkbenchStore.setState({
      phase: "ready",
      nativeTransitionEpoch: 0,
      projectGeneration: 0,
      projectTransitioning: false,
      project: { id: "project-a", name: "A", roots: [] },
    });

    await useWorkbenchStore.getState().loadRepositoryInventory();

    expect(useWorkbenchStore.getState().selectedRepository).toEqual(ref);
    expect(status).toHaveBeenCalledWith(expect.objectContaining({ ref }));
  });
});
