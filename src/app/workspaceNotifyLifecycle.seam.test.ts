import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BootstrapSnapshot } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";
import { unbindWorkspaceNotify } from "./workspaceNotifyLifecycle";

const projectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();

function snapshot(): BootstrapSnapshot {
  return {
    transitionEpoch: 7,
    projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
    project: {
      id: "project-a",
      name: "A",
      roots: [
        {
          id: "root-a",
          name: "A",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    selectedRootId: "root-a",
    selectedResource: null,
    harnesses: [],
    sessionId: null,
    selectedHarnessId: null,
    selectedModelId: null,
    messages: [],
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
  };
}

describe("live Project workspace notify bind", () => {
  beforeEach(() => {
    useWorkbenchStore.setState(initialState, true);
    vi.spyOn(projectPort, "listDirectory").mockResolvedValue({
      resource: { rootId: "root-a", relativePath: "" },
      entries: [],
      truncated: false,
    });
    vi.spyOn(projectPort, "getBootstrapSnapshot").mockResolvedValue(snapshot());
    vi.spyOn(projectPort, "watchWorkspaceNotify").mockResolvedValue({
      generation: 1,
    });
    vi.spyOn(projectPort, "unwatchWorkspaceNotify").mockResolvedValue();
  });

  afterEach(async () => {
    await unbindWorkspaceNotify();
    vi.restoreAllMocks();
    useWorkbenchStore.setState(initialState, true);
  });

  it("watches available roots after the live Project projection lands", async () => {
    await useWorkbenchStore.getState().initialize();
    expect(projectPort.watchWorkspaceNotify).toHaveBeenCalledWith({
      stamp: {
        projectId: "project-a",
        projectGeneration: 1,
        transitionEpoch: 7,
      },
      roots: [{ rootId: "root-a" }],
    });
  });

  it("still commits Project projection when watch rejects", async () => {
    vi.spyOn(projectPort, "watchWorkspaceNotify").mockRejectedValue(
      new Error("watch failed"),
    );
    await expect(
      useWorkbenchStore.getState().initialize(),
    ).resolves.toBeUndefined();
    expect(useWorkbenchStore.getState().project?.id).toBe("project-a");
    expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
  });

  it("unwatches when the workbench closes", async () => {
    vi.spyOn(projectPort, "persistProjectLayoutAndClose").mockResolvedValue();
    await useWorkbenchStore.getState().initialize();
    await useWorkbenchStore.getState().closeWorkbench();
    expect(projectPort.unwatchWorkspaceNotify).toHaveBeenCalledWith({
      projectId: "project-a",
    });
  });
});
