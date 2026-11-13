import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { childResource, resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  DirectoryListing,
  ProjectRepositoryStatus,
} from "../domain/workbench";
import {
  MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS,
  subscribeWorkspaceNotifySearch,
  syncLiveProjectWorkspaceNotify,
  type WorkspaceNotifyNotice,
} from "../features/files/workspaceNotifyCompose";
import { createNotifyRefreshes } from "../features/files/workspaceNotifyRefreshes";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { desktop } from "../services/desktop";
import {
  flushEditorSettingsPersistence,
  useEditorSettingsStore,
} from "./editorSettingsStore";
import { useWorkbenchStore } from "./store";
import {
  REFRESH_DEBOUNCE_MS,
  startRepositoryAutoRefresh,
} from "./repositoryAutoRefresh";
import {
  createWorkspaceNotifyHooks,
  liveProjectNotifyStamp,
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
    await flushEditorSettingsPersistence();
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

  it("refreshes only the listings inside an ignored folder that changed", async () => {
    await useWorkbenchStore.getState().initialize();
    const root = childResource("root-a");
    const target = { rootId: "root-a", relativePath: "target" };
    const debug = { rootId: "root-a", relativePath: "target/debug" };
    const sibling = { rootId: "root-a", relativePath: "target-notes" };
    useWorkbenchStore.setState({
      directoryListings: Object.fromEntries(
        [root, target, sibling].map((resource) => [
          resourceKey(resource),
          {
            phase: "ready" as const,
            value: { resource, entries: [], truncated: false },
          },
        ]),
      ),
      expandedDirectories: { [resourceKey(debug)]: true },
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
        relativePath: "target",
        kind: "ignored",
      },
      target,
    );
    const refreshed = listDirectory.mock.calls.map(([resource]) => resource);
    expect(refreshed).toEqual(expect.arrayContaining([target, debug]));
    expect(refreshed).toHaveLength(2);
  });

  it.each(["notify", "Explorer", "cached listing"])(
    "keeps an ignored listing current when its %s read outlasts the throttle",
    async (source) => {
      await useWorkbenchStore.getState().initialize();
      vi.useFakeTimers();
      const target = childResource("root-a", "target");
      const listing = (name: string): DirectoryListing => ({
        resource: target,
        entries: [
          {
            name,
            resource: childResource("root-a", `target/${name}`),
            kind: "file",
            byteLen: 1,
          },
        ],
        truncated: false,
      });
      const before = listing("before.txt");
      const after = listing("after.txt");
      useWorkbenchStore.setState({
        directoryListings:
          source === "Explorer"
            ? {}
            : { [resourceKey(target)]: { phase: "ready", value: before } },
        expandedDirectories:
          source === "notify" ? { [resourceKey(target)]: true } : {},
      });
      let finishRead!: (value: DirectoryListing) => void;
      const firstRead = new Promise<DirectoryListing>((resolve) => {
        finishRead = resolve;
      });
      vi.mocked(projectPort.listDirectory)
        .mockImplementationOnce(() => firstRead)
        .mockResolvedValue(after);
      const refreshes = createNotifyRefreshes(
        createWorkspaceNotifyHooks(() => useWorkbenchStore.getState())
          .invalidate,
        () => true,
        MAX_IN_FLIGHT_NOTIFY_INVALIDATIONS,
      );
      const notice = {
        projectId: "project-a",
        projectGeneration: 1,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "target",
        kind: "ignored",
      };
      try {
        if (source === "notify") {
          refreshes.ignoredFolder(notice);
        } else if (source === "Explorer") {
          useWorkbenchStore.getState().toggleDirectory(target);
        } else {
          void useWorkbenchStore.getState().loadDirectory(target, true);
        }
        await vi.advanceTimersByTimeAsync(0);
        refreshes.ignoredFolder(notice);
        // The trailing notice matures while the first native read is pending.
        await vi.advanceTimersByTimeAsync(2_500);
        finishRead(before);
        await vi.advanceTimersByTimeAsync(0);
        expect(
          useWorkbenchStore.getState().directoryListings[resourceKey(target)],
        ).toEqual({ phase: "ready", value: after });
      } finally {
        refreshes.dispose();
        vi.useRealTimers();
      }
    },
  );

  it("holds a shared listing slot through an Explorer read's trailing refresh", async () => {
    await useWorkbenchStore.getState().initialize();
    vi.useFakeTimers();
    const target = childResource("root-a", "target");
    const others = Array.from({ length: 8 }, (_, index) =>
      childResource("root-a", `other-${index}`),
    );
    useWorkbenchStore.setState({
      directoryListings: Object.fromEntries(
        others.map((resource) => [
          resourceKey(resource),
          {
            phase: "ready" as const,
            value: { resource, entries: [], truncated: false },
          },
        ]),
      ),
      expandedDirectories: {},
    });
    let inFlight = 0;
    let maxInFlight = 0;
    const finishers: Array<() => void> = [];
    vi.mocked(projectPort.listDirectory).mockImplementation(
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
    const hooks = createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    );
    const notice = {
      projectId: "project-a",
      projectGeneration: 1,
      transitionEpoch: 7,
      rootId: "root-a",
      relativePath: "target",
      kind: "ignored",
    };
    useWorkbenchStore.getState().toggleDirectory(target);
    const pending = [
      hooks.invalidate(notice, target),
      ...others.map((resource) =>
        hooks.invalidate(
          {
            ...notice,
            kind: "changed",
            relativePath: `${resource.relativePath}/file.ts`,
          },
          resource,
        ),
      ),
    ];
    try {
      await vi.advanceTimersByTimeAsync(0);
      expect(maxInFlight).toBe(8);
      // End Explorer's read while the other listings still occupy their slots.
      finishers.shift()!();
      await vi.advanceTimersByTimeAsync(0);
      expect(maxInFlight).toBe(8);
    } finally {
      while (finishers.length > 0) {
        for (const finish of finishers.splice(0)) finish();
        await vi.advanceTimersByTimeAsync(0);
      }
      await Promise.all(pending);
      vi.useRealTimers();
    }
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(target)],
    ).toEqual({
      phase: "ready",
      value: { resource: target, entries: [], truncated: false },
    });
  });

  it("refreshes tracked changes under an ignored folder without polling or a Git stamp change", async () => {
    await useWorkbenchStore.getState().initialize();
    await unbindWorkspaceNotify();
    vi.useFakeTimers();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    let emit: (value: WorkspaceNotifyNotice) => void = () => undefined;
    await syncLiveProjectWorkspaceNotify(
      liveProjectNotifyStamp(useWorkbenchStore.getState()),
      {
        ...createWorkspaceNotifyHooks(() => useWorkbenchStore.getState()),
        async listen(listener) {
          emit = listener;
          return () => undefined;
        },
      },
    );
    const modified: ProjectRepositoryStatus = {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "main",
          changes: [
            {
              resource: childResource("root-a", "dist/app.js"),
              indexStatus: "unchanged",
              worktreeStatus: "modified",
              additions: 1,
              deletions: 0,
            },
          ],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    };
    vi.spyOn(desktop, "getRepositoryStatus").mockResolvedValue(modified);
    const stop = startRepositoryAutoRefresh({
      async refresh() {
        await useWorkbenchStore.getState().refreshRepository();
        return "changed";
      },
      changeStamp: async () => "fixed-git-metadata",
      subscribeNotices: subscribeWorkspaceNotifySearch,
      pollAlways: false,
    });
    try {
      await vi.advanceTimersByTimeAsync(1_000);
      expect(useWorkbenchStore.getState().repositories.roots).toEqual([]);
      emit({
        projectId: "project-a",
        projectGeneration: 1,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "dist",
        kind: "ignored",
      });
      await vi.advanceTimersByTimeAsync(REFRESH_DEBOUNCE_MS);
      expect(useWorkbenchStore.getState().repositories).toEqual(modified);
    } finally {
      stop();
      await unbindWorkspaceNotify();
      vi.useRealTimers();
    }
  });

  it("refreshes the root's cached listings when an ignore file changes", async () => {
    await useWorkbenchStore.getState().initialize();
    const root = childResource("root-a");
    const src = { rootId: "root-a", relativePath: "src" };
    const deep = { rootId: "root-a", relativePath: "src/deep" };
    useWorkbenchStore.setState({
      directoryListings: Object.fromEntries(
        [root, src, deep].map((resource) => [
          resourceKey(resource),
          {
            phase: "ready" as const,
            value: { resource, entries: [], truncated: false },
          },
        ]),
      ),
      expandedDirectories: {},
    });
    const listDirectory = vi.mocked(projectPort.listDirectory);
    const hooks = createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    );
    const notice = (relativePath: string) => ({
      projectId: "project-a",
      projectGeneration: 1,
      transitionEpoch: 7,
      rootId: "root-a",
      relativePath,
      kind: "modify",
    });
    listDirectory.mockClear();
    await hooks.invalidate(notice("src/.gitignore"), src);
    expect(listDirectory.mock.calls.map(([resource]) => resource)).toEqual(
      expect.arrayContaining([root, src, deep]),
    );
    listDirectory.mockClear();
    await hooks.invalidate(notice("src/notes.gitignore.txt"), src);
    expect(listDirectory.mock.calls.map(([resource]) => resource)).toEqual([
      src,
    ]);
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

  it("keeps overflow resync inside the shared eight listing slots", async () => {
    await useWorkbenchStore.getState().initialize();
    const resyncOnly = Array.from({ length: 8 }, (_, index) => ({
      rootId: "root-a",
      relativePath: `resync-${index}`,
    }));
    const ordinary = Array.from({ length: 8 }, (_, index) => ({
      rootId: "root-a",
      relativePath: `ordinary-${index}`,
    }));
    useWorkbenchStore.setState({
      directoryListings: Object.fromEntries(
        [...resyncOnly, ...ordinary].map((resource) => [
          resourceKey(resource),
          {
            phase: "ready" as const,
            value: { resource, entries: [], truncated: false },
          },
        ]),
      ),
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
    const hooks = createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    );
    const ordinaryPending = ordinary.map((listing) =>
      Promise.resolve(
        hooks.invalidate(
          {
            projectId: "project-a",
            projectGeneration: 1,
            transitionEpoch: 7,
            rootId: "root-a",
            relativePath: `${listing.relativePath}/file.ts`,
            kind: "changed",
          },
          listing,
        ),
      ),
    );
    await Promise.resolve();
    expect(listDirectory).toHaveBeenCalledTimes(8);
    expect(maxInFlight).toBe(8);
    const resyncPending = hooks.invalidate(
      {
        projectId: "project-a",
        projectGeneration: 1,
        transitionEpoch: 7,
        rootId: "root-a",
        relativePath: "lost-notices",
        kind: "resync",
      },
      { rootId: "root-a", relativePath: "" },
    );
    await Promise.resolve();
    expect(listDirectory).toHaveBeenCalledTimes(8);
    expect(maxInFlight).toBe(8);
    const firstBatch = finishers.splice(0);
    for (const finish of firstBatch) {
      finish();
    }
    await Promise.all(ordinaryPending);
    await vi.waitFor(() => {
      expect(listDirectory.mock.calls.length).toBeGreaterThan(8);
    });
    expect(maxInFlight).toBe(8);
    while (finishers.length > 0) {
      const batch = finishers.splice(0);
      for (const finish of batch) {
        finish();
      }
      await Promise.resolve();
    }
    await resyncPending;
    expect(maxInFlight).toBe(8);
    expect(listDirectory).toHaveBeenCalledWith(resyncOnly[0]);
  });

  it("keeps hanging listing slots after unbind so rebind cannot stack eight more", async () => {
    await useWorkbenchStore.getState().initialize();
    const firstWave = Array.from({ length: 8 }, (_, index) => ({
      rootId: "root-a",
      relativePath: `first-${index}`,
    }));
    const secondWave = Array.from({ length: 8 }, (_, index) => ({
      rootId: "root-a",
      relativePath: `second-${index}`,
    }));
    useWorkbenchStore.setState({
      directoryListings: Object.fromEntries(
        [...firstWave, ...secondWave].map((resource) => [
          resourceKey(resource),
          {
            phase: "ready" as const,
            value: { resource, entries: [], truncated: false },
          },
        ]),
      ),
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
    const firstHooks = createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    );
    const firstPending = firstWave.map((listing) =>
      Promise.resolve(
        firstHooks.invalidate(
          {
            projectId: "project-a",
            projectGeneration: 1,
            transitionEpoch: 7,
            rootId: "root-a",
            relativePath: `${listing.relativePath}/file.ts`,
            kind: "changed",
          },
          listing,
        ),
      ),
    );
    await Promise.resolve();
    expect(listDirectory).toHaveBeenCalledTimes(8);
    expect(maxInFlight).toBe(8);
    await unbindWorkspaceNotify();
    const reboundHooks = createWorkspaceNotifyHooks(() =>
      useWorkbenchStore.getState(),
    );
    const secondPending = secondWave.map((listing) =>
      Promise.resolve(
        reboundHooks.invalidate(
          {
            projectId: "project-a",
            projectGeneration: 1,
            transitionEpoch: 7,
            rootId: "root-a",
            relativePath: `${listing.relativePath}/file.ts`,
            kind: "changed",
          },
          listing,
        ),
      ),
    );
    await Promise.resolve();
    expect(listDirectory).toHaveBeenCalledTimes(8);
    expect(maxInFlight).toBe(8);
    const firstBatch = finishers.splice(0);
    for (const finish of firstBatch) {
      finish();
    }
    await Promise.all(firstPending);
    await vi.waitFor(() => {
      expect(listDirectory.mock.calls.length).toBeGreaterThan(8);
    });
    expect(maxInFlight).toBe(8);
    while (finishers.length > 0) {
      const batch = finishers.splice(0);
      for (const finish of batch) {
        finish();
      }
      await Promise.resolve();
    }
    await Promise.all(secondPending);
    expect(maxInFlight).toBe(8);
    expect(listDirectory).toHaveBeenCalledWith(secondWave[0]);
  });

  it("unwatches when the workbench closes", async () => {
    vi.spyOn(projectPort, "persistProjectLayoutAndClose").mockResolvedValue();
    await useWorkbenchStore.getState().initialize();
    await useWorkbenchStore.getState().closeWorkbench();
    expect(projectPort.unwatchWorkspaceNotify).toHaveBeenCalledWith({
      projectId: "project-a",
    });
  });

  it("identifies the failed close stage without issuing native destroy", async () => {
    await useWorkbenchStore.getState().initialize();
    useWorkbenchStore.setState({
      flushActiveEditor: vi
        .fn()
        .mockRejectedValue(new Error("snapshot failed")),
    });
    const close = vi.spyOn(projectPort, "persistProjectLayoutAndClose");

    await expect(useWorkbenchStore.getState().closeWorkbench()).rejects.toThrow(
      "Close editorFlush failed: snapshot failed",
    );
    expect(close).not.toHaveBeenCalled();
  });

  it("waits for the latest editor settings before native close", async () => {
    let finishSave: (() => void) | undefined;
    let finishUnwatch: (() => void) | undefined;
    const save = vi
      .spyOn(getWorkbenchPorts().settings, "setEditorSettings")
      .mockImplementation(
        () =>
          new Promise((resolve) => {
            finishSave = () => resolve({ "editor.fontSize": 16 });
          }),
      );
    const close = vi
      .spyOn(projectPort, "persistProjectLayoutAndClose")
      .mockResolvedValue();
    vi.mocked(projectPort.unwatchWorkspaceNotify).mockImplementation(
      () =>
        new Promise((resolve) => {
          finishUnwatch = resolve;
        }),
    );
    await useWorkbenchStore.getState().initialize();
    useEditorSettingsStore.setState({ persistenceReady: true });
    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 16 });

    const closing = useWorkbenchStore.getState().closeWorkbench();
    await vi.waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(close).not.toHaveBeenCalled();

    finishSave?.();
    await vi.waitFor(() => expect(finishUnwatch).toBeTypeOf("function"));
    useEditorSettingsStore.getState().setSettings({ "editor.fontSize": 14 });
    useEditorSettingsStore.getState().retryPersistence();
    expect(save).toHaveBeenCalledOnce();
    expect(useEditorSettingsStore.getState().settings["editor.fontSize"]).toBe(
      16,
    );
    expect(close).not.toHaveBeenCalled();

    finishUnwatch?.();
    await closing;
    expect(close).toHaveBeenCalledOnce();
    expect(useEditorSettingsStore.getState().persistenceClosing).toBe(false);
  });
});
