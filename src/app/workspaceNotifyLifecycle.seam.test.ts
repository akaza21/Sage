import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { childResource, resourceKey } from "../domain/resourceIdentity";
import type { BootstrapSnapshot } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";
import {
  createWorkspaceNotifyHooks,
  unbindWorkspaceNotify,
} from "./workspaceNotifyLifecycle";

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

  it("refreshes cached listings for the root on a resync notice", async () => {
    await useWorkbenchStore.getState().initialize();
    const root = childResource("root-a");
    const nested = { rootId: "root-a", relativePath: "src" };
    useWorkbenchStore.setState({
      directoryListings: {
        [resourceKey(root)]: {
          phase: "ready",
          value: { resource: root, entries: [], truncated: false },
        },
        [resourceKey(nested)]: {
          phase: "ready",
          value: { resource: nested, entries: [], truncated: false },
        },
      },
      expandedDirectories: { [resourceKey(nested)]: true },
    });
    const listDirectory = vi.mocked(projectPort.listDirectory);
    listDirectory.mockClear();
    await createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    ).invalidate(
      {
        projectId: "project-a",
        projectGeneration: 1,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "lost-notices",
        kind: "resync",
      },
      root,
    );
    const refreshed = listDirectory.mock.calls.map(([resource]) => resource);
    expect(refreshed).toEqual(expect.arrayContaining([root, nested]));
    expect(refreshed).toHaveLength(2);
  });

  it("refreshes more than eight cached listings for one resync in batches", async () => {
    await useWorkbenchStore.getState().initialize();
    const listings = Array.from({ length: 8 }, (_, index) => ({
      rootId: "root-a",
      relativePath: index === 0 ? "" : `dir-${index}`,
    }));
    const expandedOnly = { rootId: "root-a", relativePath: "expanded-only" };
    const directoryListings = Object.fromEntries(
      listings.map((resource) => [
        resourceKey(resource),
        {
          phase: "ready" as const,
          value: { resource, entries: [], truncated: false },
        },
      ]),
    );
    useWorkbenchStore.setState({
      directoryListings,
      expandedDirectories: { [resourceKey(expandedOnly)]: true },
    });
    const listDirectory = vi.mocked(projectPort.listDirectory);
    let inFlight = 0;
    let maxInFlight = 0;
    const finishers: Array<() => void> = [];
    listDirectory.mockImplementation(
      (resource) =>
        new Promise((resolve) => {
          inFlight += 1;
          maxInFlight = Math.max(maxInFlight, inFlight);
          finishers.push(() => {
            inFlight -= 1;
            resolve({ resource, entries: [], truncated: false });
          });
        }),
    );
    listDirectory.mockClear();
    const pending = createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    ).invalidate(
      {
        projectId: "project-a",
        projectGeneration: 1,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "lost-notices",
        kind: "resync",
      },
      listings[0],
    );
    await Promise.resolve();
    expect(listDirectory).toHaveBeenCalledTimes(8);
    expect(
      listDirectory.mock.calls.map(([resource]) => resource),
    ).not.toContainEqual(expandedOnly);
    expect(maxInFlight).toBe(8);
    const firstBatch = finishers.splice(0);
    for (const finish of firstBatch) {
      finish();
    }
    await vi.waitFor(() => {
      expect(listDirectory).toHaveBeenCalledTimes(9);
    });
    expect(listDirectory).toHaveBeenCalledWith(expandedOnly);
    for (const finish of finishers.splice(0)) {
      finish();
    }
    await pending;
    expect(maxInFlight).toBe(8);
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
