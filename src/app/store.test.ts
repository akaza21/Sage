import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  BufferSnapshotAck,
  DirectoryListing,
  EditorDocument,
  FileDiff,
  ModelOption,
  ProjectRepositoryStatus,
  ResourceRef,
  WorkbenchTab,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

const resource = (relativePath: string, rootId = "root-a"): ResourceRef => ({
  rootId,
  relativePath,
});
const chatTab: WorkbenchTab = {
  id: "chat:a",
  kind: "chat",
  title: "Chat: A",
  position: 0,
  pinned: true,
  dirty: false,
};

function document(
  target: ResourceRef,
  content: string,
  revision: number,
  dirty: boolean,
): EditorDocument {
  const version = {
    fingerprint: dirty ? "base" : `saved-${revision}`,
    byteLen: content.length,
    modifiedAt: null,
  };
  return {
    resource: target,
    disk: version,
    base: version,
    buffer: {
      fingerprint: dirty ? `buffer-${revision}` : version.fingerprint,
      byteLen: content.length,
      revision,
      content,
    },
    lineEnding: "lf",
    dirty,
    conflict: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function snapshotB(): BootstrapSnapshot {
  return {
    transitionEpoch: 1,
    projects: [
      { id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 },
      { id: "project-b", name: "B", rootCount: 1, lastOpenedAt: 2 },
    ],
    project: {
      id: "project-b",
      name: "B",
      roots: [
        {
          id: "root-b",
          name: "B",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    tabs: [
      {
        id: "chat:b",
        kind: "chat",
        title: "Chat: B",
        position: 0,
        pinned: true,
        dirty: false,
      },
    ],
    activeTabId: "chat:b",
    selectedRootId: "root-b",
    selectedResource: null,
    harnesses: [],
    sessionId: "session-b",
    selectedHarnessId: null,
    selectedModelId: null,
    messages: [],
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
    turnChanges: [],
    recoveredBuffers: [],
    recoveryNotice: null,
  };
}

describe("workbench resource and project navigation", () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      phase: "ready",
      nativeTransitionEpoch: 0,
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
          {
            id: "root-b",
            name: "B",
            ordinal: 1,
            availability: "available",
            message: null,
          },
        ],
      },
      tabs: [chatTab],
      activeTabId: chatTab.id,
      selectedRootId: "root-a",
      selectedResource: null,
      sidebarOpen: false,
      sidebarView: "sourceControl",
      error: null,
      projectGeneration: 0,
      projectTransitioning: false,
      directoryListings: {},
      expandedDirectories: {},
      activeEditor: null,
      pendingCloseTabId: null,
      diffs: {},
      requestGenerations: {},
      requestIntentSequence: 0,
      repositories: { roots: [], truncated: false, elapsedMs: 0 },
      repositoryRefreshing: false,
      harnesses: [],
      selectedHarnessId: null,
      selectedModelId: null,
      models: [],
      sessionId: "session-a",
      messages: [],
      recoveryNotice: null,
    });
  });

  afterEach(() => vi.restoreAllMocks());

  it("keeps the same relative path in two roots as distinct tabs and caches", async () => {
    const left = resource("src/main.rs", "root-a");
    const right = resource("src/main.rs", "root-b");
    await useWorkbenchStore.getState().openFile(left);
    await useWorkbenchStore.getState().openFile(right);

    const files = useWorkbenchStore
      .getState()
      .tabs.filter((tab) => tab.kind === "file");
    expect(files).toHaveLength(2);
    expect(files.map((tab) => tab.resource?.rootId)).toEqual([
      "root-a",
      "root-b",
    ]);
    expect(resourceKey(left)).not.toBe(resourceKey(right));
  });

  it("opens a changed resource as a diff and focuses Explorer", async () => {
    const target = resource("src/app/store.ts");
    await useWorkbenchStore.getState().openDiff(target);
    const state = useWorkbenchStore.getState();
    expect(state.tabs).toContainEqual(
      expect.objectContaining({ kind: "diff", resource: target }),
    );
    expect(state.selectedResource).toEqual(target);
    expect(state.sidebarOpen).toBe(true);
    expect(state.sidebarView).toBe("explorer");
  });

  it("never evicts dirty tabs at the resource-tab cap", async () => {
    const dirtyTabs: WorkbenchTab[] = Array.from(
      { length: 32 },
      (_, index) => ({
        id: `file:${index}`,
        kind: "file",
        title: `${index}.txt`,
        position: index + 1,
        pinned: false,
        dirty: true,
        resource: resource(`${index}.txt`),
      }),
    );
    useWorkbenchStore.setState({ tabs: [chatTab, ...dirtyTabs] });

    await useWorkbenchStore.getState().openFile(resource("overflow.txt"));

    expect(
      useWorkbenchStore.getState().tabs.filter((tab) => tab.kind === "file"),
    ).toHaveLength(32);
    expect(useWorkbenchStore.getState().error).toContain(
      "32 resource-tab limit",
    );
  });

  it("requires an explicit decision before closing a dirty tab", async () => {
    await useWorkbenchStore
      .getState()
      .openFile(resource("src-tauri/src/main.rs"));
    useWorkbenchStore.getState().updateEditorContent("dirty");
    const tabId = useWorkbenchStore.getState().activeTabId!;
    useWorkbenchStore.getState().closeTab(tabId);
    expect(useWorkbenchStore.getState().pendingCloseTabId).toBe(tabId);
    expect(
      useWorkbenchStore.getState().tabs.some((tab) => tab.id === tabId),
    ).toBe(true);
  });

  it("serializes overlapping snapshots and saves the latest revision", async () => {
    const target = resource("race.txt");
    const tab: WorkbenchTab = {
      id: "file:race",
      kind: "file",
      title: "race.txt",
      position: 1,
      pinned: false,
      dirty: false,
      resource: target,
    };
    const first = deferred<BufferSnapshotAck>();
    const requests: string[] = [];
    vi.spyOn(desktop, "snapshotEditorBuffer").mockImplementation(
      async (request) => {
        requests.push(request.content);
        return requests.length === 1
          ? first.promise
          : {
              revision: request.revision,
              persistedAt: 2,
              dirty: true,
            };
      },
    );
    vi.spyOn(desktop, "saveEditorDocument").mockResolvedValue({
      outcome: "saved",
      document: document(target, "latest\n", 2, false),
    });
    vi.spyOn(desktop, "getRepositoryStatus").mockResolvedValue({
      roots: [],
      truncated: false,
      elapsedMs: 0,
    });
    useWorkbenchStore.setState({
      tabs: [chatTab, tab],
      activeTabId: tab.id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base\n", 0, false),
        content: "base\n",
        revision: 0,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    useWorkbenchStore.getState().updateEditorContent("first\n");
    const timerFlush = useWorkbenchStore.getState().flushActiveEditor();
    await vi.waitFor(() => expect(requests).toEqual(["first\n"]));
    useWorkbenchStore.getState().updateEditorContent("latest\n");
    const save = useWorkbenchStore.getState().saveActiveEditor();
    first.resolve({ revision: 1, persistedAt: 1, dirty: true });
    await Promise.all([timerFlush, save]);
    expect(requests).toEqual(["first\n", "latest\n"]);
  });

  it("blocks project switching when the newest snapshot fails", async () => {
    const target = resource("dirty.txt");
    vi.spyOn(desktop, "snapshotEditorBuffer").mockRejectedValue(
      new Error("disk unavailable"),
    );
    const switchNative = vi.spyOn(desktop, "switchProject");
    useWorkbenchStore.setState({
      tabs: [
        chatTab,
        {
          id: "file:dirty",
          kind: "file",
          title: "dirty.txt",
          position: 1,
          pinned: false,
          dirty: true,
          resource: target,
        },
      ],
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    await useWorkbenchStore.getState().switchProject("project-b");

    expect(switchNative).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().project?.id).toBe("project-a");
  });

  it("clears prior project caches only after a successful switch", async () => {
    const oldRoot = resource("");
    useWorkbenchStore.setState({
      directoryListings: {
        [resourceKey(oldRoot)]: {
          phase: "ready",
          value: { resource: oldRoot, entries: [], truncated: false },
        },
      },
    });
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));

    await useWorkbenchStore.getState().switchProject("project-b");

    const state = useWorkbenchStore.getState();
    expect(state.project?.id).toBe("project-b");
    expect(state.directoryListings[resourceKey(oldRoot)]).toBeUndefined();
    expect(
      state.directoryListings[resourceKey(resource("", "root-b"))],
    ).toEqual(expect.objectContaining({ phase: "ready" }));
  });

  it("reuses an existing File tab for the same resource", async () => {
    const target = resource("src/reused.ts");
    await useWorkbenchStore.getState().openFile(target);
    const firstId = useWorkbenchStore.getState().activeTabId;
    await useWorkbenchStore.getState().openFile(target);
    expect(useWorkbenchStore.getState().activeTabId).toBe(firstId);
    expect(
      useWorkbenchStore
        .getState()
        .tabs.filter(
          (tab) =>
            tab.kind === "file" &&
            tab.resource?.rootId === target.rootId &&
            tab.resource.relativePath === target.relativePath,
        ),
    ).toHaveLength(1);
  });

  it("reuses an existing Diff tab for the same resource", async () => {
    const target = resource("src/reused.ts");
    await useWorkbenchStore.getState().openDiff(target);
    const firstId = useWorkbenchStore.getState().activeTabId;
    await useWorkbenchStore.getState().openDiff(target);
    expect(useWorkbenchStore.getState().activeTabId).toBe(firstId);
    expect(
      useWorkbenchStore
        .getState()
        .tabs.filter(
          (tab) =>
            tab.kind === "diff" &&
            tab.resource?.rootId === target.rootId &&
            tab.resource.relativePath === target.relativePath,
        ),
    ).toHaveLength(1);
  });

  it("flushes the latest dirty editor revision before activating another tab", async () => {
    const target = resource("switch.txt");
    const fileTab: WorkbenchTab = {
      id: "file:switch",
      kind: "file",
      title: "switch.txt",
      position: 1,
      pinned: false,
      dirty: true,
      resource: target,
    };
    const pending = deferred<BufferSnapshotAck>();
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockReturnValue(pending.promise);
    useWorkbenchStore.setState({
      tabs: [chatTab, fileTab],
      activeTabId: fileTab.id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const activation = useWorkbenchStore.getState().activateTab(chatTab.id);
    await vi.waitFor(() => expect(snapshot).toHaveBeenCalledOnce());
    expect(useWorkbenchStore.getState().activeTabId).toBe(fileTab.id);
    pending.resolve({ revision: 1, persistedAt: 1, dirty: true });
    await activation;
    expect(useWorkbenchStore.getState().activeTabId).toBe(chatTab.id);
  });

  it("refreshes both Explorer and Source Control after a successful save", async () => {
    const target = resource("saved.txt");
    const fileTab: WorkbenchTab = {
      id: "file:saved",
      kind: "file",
      title: "saved.txt",
      position: 1,
      pinned: false,
      dirty: true,
      resource: target,
    };
    vi.spyOn(desktop, "snapshotEditorBuffer").mockResolvedValue({
      revision: 1,
      persistedAt: 1,
      dirty: true,
    });
    vi.spyOn(desktop, "saveEditorDocument").mockResolvedValue({
      outcome: "saved",
      document: document(target, "saved", 1, false),
    });
    const repository = vi
      .spyOn(desktop, "getRepositoryStatus")
      .mockResolvedValue({ roots: [], truncated: false, elapsedMs: 1 });
    const directory = vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource(""),
      entries: [],
      truncated: false,
    });
    useWorkbenchStore.setState({
      tabs: [chatTab, fileTab],
      activeTabId: fileTab.id,
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "saved",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    expect(await useWorkbenchStore.getState().saveActiveEditor()).toBe(true);
    expect(repository).toHaveBeenCalledOnce();
    expect(directory).toHaveBeenCalledWith(resource(""));
  });

  it.each(["success", "error"] as const)(
    "ignores a late old-project directory %s after switching",
    async (outcome) => {
      const target = resource("src");
      const pending = deferred<DirectoryListing>();
      vi.spyOn(desktop, "listDirectory").mockImplementation((requested) =>
        requested.rootId === "root-a"
          ? pending.promise
          : Promise.resolve({
              resource: requested,
              entries: [],
              truncated: false,
            }),
      );
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());

      const oldLoad = useWorkbenchStore.getState().loadDirectory(target);
      await vi.waitFor(() =>
        expect(
          useWorkbenchStore.getState().directoryListings[resourceKey(target)],
        ).toEqual({ phase: "loading" }),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve({ resource: target, entries: [], truncated: false });
      } else {
        pending.reject(new Error("old directory failed"));
      }
      await oldLoad;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.directoryListings[resourceKey(target)]).toBeUndefined();
      expect(state.error).not.toBe("old directory failed");
    },
  );

  it.each(["success", "error"] as const)(
    "ignores a late old-project diff %s after switching",
    async (outcome) => {
      const target = resource("old.ts");
      const diffTab: WorkbenchTab = {
        id: "diff:old",
        kind: "diff",
        title: "old.ts",
        position: 1,
        pinned: false,
        dirty: false,
        resource: target,
      };
      const pending = deferred<FileDiff>();
      vi.spyOn(desktop, "getFileDiff").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());
      useWorkbenchStore.setState({
        tabs: [chatTab, diffTab],
        activeTabId: diffTab.id,
      });

      const oldLoad = useWorkbenchStore.getState().loadDiff(target);
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().diffs[resourceKey(target)]).toEqual(
          { phase: "loading" },
        ),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve({
          resource: target,
          hunks: [],
          binary: false,
          truncated: false,
        });
      } else {
        pending.reject(new Error("old diff failed"));
      }
      await oldLoad;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.diffs[resourceKey(target)]).toBeUndefined();
      expect(state.error).not.toBe("old diff failed");
    },
  );

  it.each(["success", "error"] as const)(
    "ignores a late old-project editor %s after switching",
    async (outcome) => {
      const target = resource("old.txt");
      const fileTab: WorkbenchTab = {
        id: "file:old",
        kind: "file",
        title: "old.txt",
        position: 1,
        pinned: false,
        dirty: false,
        resource: target,
      };
      const pending = deferred<EditorDocument>();
      vi.spyOn(desktop, "openEditorDocument").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());
      useWorkbenchStore.setState({
        tabs: [chatTab, fileTab],
        activeTabId: fileTab.id,
      });

      const oldLoad = useWorkbenchStore.getState().loadEditor(target);
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().activeEditor).toEqual({
          phase: "loading",
          resource: target,
        }),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve(document(target, "old", 1, false));
      } else {
        pending.reject(new Error("old editor failed"));
      }
      await oldLoad;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.activeEditor).toBeNull();
      expect(state.error).not.toBe("old editor failed");
    },
  );

  it.each(["success", "error"] as const)(
    "ignores an old repository refresh %s after switching",
    async (outcome) => {
      const pending = deferred<ProjectRepositoryStatus>();
      vi.spyOn(desktop, "getRepositoryStatus").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: null,
        recoveryError: null,
      });
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(snapshotB());

      const oldRefresh = useWorkbenchStore.getState().refreshRepository();
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().repositoryRefreshing).toBe(true),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve({
          roots: [
            {
              rootId: "root-a",
              state: "repository",
              branch: "old",
              changes: [],
              truncated: false,
              elapsedMs: 1,
              message: null,
            },
          ],
          truncated: false,
          elapsedMs: 1,
        });
      } else {
        pending.reject(new Error("old repository failed"));
      }
      await oldRefresh;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.repositories).toEqual(snapshotB().repositories);
      expect(state.repositoryRefreshing).toBe(false);
      expect(state.error).not.toBe("old repository failed");
    },
  );

  it("keeps the newest harness/model intent when an older load finishes last", async () => {
    const oldModels = deferred<ModelOption[]>();
    vi.spyOn(desktop, "listHarnessModels").mockImplementation((harnessId) =>
      harnessId === "old"
        ? oldModels.promise
        : Promise.resolve([{ id: "new-model", name: "New" }]),
    );

    const oldSelection = useWorkbenchStore.getState().selectHarness("old");
    await useWorkbenchStore.getState().selectHarness("new");
    oldModels.resolve([{ id: "old-model", name: "Old" }]);
    await oldSelection;

    const state = useWorkbenchStore.getState();
    expect(state.selectedHarnessId).toBe("new");
    expect(state.models).toEqual([{ id: "new-model", name: "New" }]);
    expect(state.selectedModelId).toBe("new-model");
  });

  it.each(["success", "error"] as const)(
    "ignores late harness/model %s after switching projects",
    async (outcome) => {
      const pending = deferred<ModelOption[]>();
      vi.spyOn(desktop, "listHarnessModels").mockReturnValue(pending.promise);
      vi.spyOn(desktop, "switchProject").mockResolvedValue({
        committed: true,
        transitionEpoch: 1,
        snapshot: snapshotB(),
        recoveryError: null,
      });
      vi.spyOn(desktop, "listDirectory").mockResolvedValue({
        resource: resource("", "root-b"),
        entries: [],
        truncated: false,
      });

      const oldSelection = useWorkbenchStore.getState().selectHarness("old");
      await vi.waitFor(() =>
        expect(useWorkbenchStore.getState().selectedHarnessId).toBe("old"),
      );
      await useWorkbenchStore.getState().switchProject("project-b");
      if (outcome === "success") {
        pending.resolve([{ id: "stale", name: "Stale" }]);
      } else {
        pending.reject(new Error("old harness failed"));
      }
      await oldSelection;

      const state = useWorkbenchStore.getState();
      expect(state.project?.id).toBe("project-b");
      expect(state.models).toEqual([]);
      expect(state.error).not.toBe("old harness failed");
    },
  );

  it("ignores a rejected same-harness ABA completion", async () => {
    const old = deferred<ModelOption[]>();
    let oldCalls = 0;
    vi.spyOn(desktop, "listHarnessModels").mockImplementation((harnessId) => {
      if (harnessId === "old" && oldCalls++ === 0) return old.promise;
      return Promise.resolve([{ id: `${harnessId}-new`, name: "New" }]);
    });
    const first = useWorkbenchStore.getState().selectHarness("old");
    await useWorkbenchStore.getState().selectHarness("new");
    await useWorkbenchStore.getState().selectHarness("old");
    old.reject(new Error("stale ABA failure"));
    await first;
    expect(useWorkbenchStore.getState().selectedHarnessId).toBe("old");
    expect(useWorkbenchStore.getState().selectedModelId).toBe("old-new");
    expect(useWorkbenchStore.getState().error).not.toBe("stale ABA failure");
  });

  it("restores persisted harness models during initialization", async () => {
    const restored = snapshotB();
    restored.selectedHarnessId = "saved-harness";
    restored.selectedModelId = "saved-model";
    vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(restored);
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    const models = vi.spyOn(desktop, "listHarnessModels").mockResolvedValue([
      { id: "fallback-model", name: "Fallback" },
      { id: "saved-model", name: "Saved" },
    ]);

    await useWorkbenchStore.getState().initialize();

    expect(models).toHaveBeenCalledWith("saved-harness");
    expect(useWorkbenchStore.getState()).toEqual(
      expect.objectContaining({
        projectTransitioning: false,
        selectedHarnessId: "saved-harness",
        selectedModelId: "saved-model",
      }),
    );
  });

  it("restores persisted harness models inside a project switch", async () => {
    const restored = snapshotB();
    restored.selectedHarnessId = "saved-harness";
    restored.selectedModelId = "saved-model";
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: restored,
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    const pending = deferred<ModelOption[]>();
    vi.spyOn(desktop, "listHarnessModels").mockReturnValue(pending.promise);

    const switching = useWorkbenchStore.getState().switchProject("project-b");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState()).toEqual(
        expect.objectContaining({
          projectTransitioning: true,
          selectedHarnessId: "saved-harness",
          selectedModelId: null,
        }),
      ),
    );
    pending.resolve([{ id: "saved-model", name: "Saved" }]);
    await switching;

    expect(useWorkbenchStore.getState()).toEqual(
      expect.objectContaining({
        projectTransitioning: false,
        selectedHarnessId: "saved-harness",
        selectedModelId: "saved-model",
      }),
    );
  });

  it("blocks the workbench if projection recovery fails after a committed switch", async () => {
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("bootstrap unavailable"));

    await useWorkbenchStore.getState().switchProject("project-b");

    const state = useWorkbenchStore.getState();
    expect(bootstrap).toHaveBeenCalledTimes(2);
    expect(state.phase).toBe("error");
    expect(state.error).toContain("project change committed");
    expect(state.projectTransitioning).toBe(false);
  });

  it("blocks the workbench if projection recovery fails after project creation", async () => {
    vi.spyOn(desktop, "createProjectFromPicker").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("bootstrap unavailable"));

    await useWorkbenchStore.getState().createProject();

    const state = useWorkbenchStore.getState();
    expect(bootstrap).toHaveBeenCalledTimes(2);
    expect(state.phase).toBe("error");
    expect(state.error).toContain("project change committed");
  });

  it("blocks the workbench if projection recovery fails after adding a root", async () => {
    vi.spyOn(desktop, "addProjectRootFromPicker").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: null,
      recoveryError: null,
    });
    const bootstrap = vi
      .spyOn(desktop, "getBootstrapSnapshot")
      .mockRejectedValue(new Error("bootstrap unavailable"));

    await useWorkbenchStore.getState().addProjectRoot();

    const state = useWorkbenchStore.getState();
    expect(bootstrap).toHaveBeenCalledTimes(2);
    expect(state.phase).toBe("error");
    expect(state.error).toContain("project change committed");
  });

  it("uses an authoritative snapshot returned by the switch command without a second bootstrap", async () => {
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: snapshotB(),
      recoveryError: null,
    });
    const bootstrap = vi.spyOn(desktop, "getBootstrapSnapshot");
    vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource("", "root-b"),
      entries: [],
      truncated: false,
    });

    await useWorkbenchStore.getState().switchProject("project-b");

    expect(useWorkbenchStore.getState().project?.id).toBe("project-b");
    expect(bootstrap).not.toHaveBeenCalled();
  });

  it.each(["switch", "create", "addRoot"] as const)(
    "authoritatively recovers an unknown %s commit outcome",
    async (operation) => {
      const recovered = snapshotB();
      if (operation === "addRoot") {
        recovered.project = {
          id: "project-a",
          name: "A",
          roots: [
            ...useWorkbenchStore.getState().project!.roots,
            {
              id: "root-new",
              name: "New",
              ordinal: 2,
              availability: "available",
              message: null,
            },
          ],
        };
        recovered.tabs = [chatTab];
        recovered.activeTabId = chatTab.id;
        recovered.selectedRootId = "root-a";
        recovered.sessionId = "session-a";
      }
      vi.spyOn(desktop, "getBootstrapSnapshot").mockResolvedValue(recovered);
      vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
        resource: target,
        entries: [],
        truncated: false,
      }));
      if (operation === "switch") {
        vi.spyOn(desktop, "switchProject").mockRejectedValue(
          new Error("response lost"),
        );
        await useWorkbenchStore.getState().switchProject("project-b");
      } else if (operation === "create") {
        vi.spyOn(desktop, "createProjectFromPicker").mockRejectedValue(
          new Error("response lost"),
        );
        await useWorkbenchStore.getState().createProject();
      } else {
        vi.spyOn(desktop, "addProjectRootFromPicker").mockRejectedValue(
          new Error("response lost"),
        );
        await useWorkbenchStore.getState().addProjectRoot();
      }

      expect(useWorkbenchStore.getState().phase).toBe("ready");
      expect(useWorkbenchStore.getState().project?.id).toBe(
        operation === "addRoot" ? "project-a" : "project-b",
      );
      if (operation === "addRoot") {
        expect(useWorkbenchStore.getState().project?.roots).toHaveLength(3);
      }
    },
  );

  it("rejects old project-scoped actions while a transition flush is pending", async () => {
    const target = resource("old-action.txt");
    const pending = deferred<BufferSnapshotAck>();
    vi.spyOn(desktop, "snapshotEditorBuffer").mockReturnValue(pending.promise);
    const open = vi.spyOn(desktop, "openEditorDocument");
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: snapshotB(),
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource("", "root-b"),
      entries: [],
      truncated: false,
    });
    useWorkbenchStore.setState({
      tabs: [
        chatTab,
        {
          id: "file:dirty-transition",
          kind: "file",
          title: "dirty.txt",
          position: 1,
          pinned: false,
          dirty: true,
          resource: target,
        },
      ],
      activeTabId: "file:dirty-transition",
      activeEditor: {
        phase: "ready",
        resource: target,
        document: document(target, "base", 0, false),
        content: "dirty",
        revision: 1,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });

    const switching = useWorkbenchStore.getState().switchProject("project-b");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(true),
    );
    await useWorkbenchStore.getState().openFile(resource("another-old.txt"));
    expect(open).not.toHaveBeenCalled();
    pending.resolve({ revision: 1, persistedAt: 1, dirty: true });
    await switching;
    expect(useWorkbenchStore.getState().project?.id).toBe("project-b");
  });

  it("keeps a newer directory request when an older same-resource request finishes last", async () => {
    const target = resource("");
    const old = deferred<DirectoryListing>();
    let calls = 0;
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (requested) => {
      calls += 1;
      if (calls === 1) return old.promise;
      return {
        resource: requested,
        entries: [
          {
            name: "new.txt",
            resource: resource("new.txt"),
            kind: "file",
            byteLen: 1,
          },
        ],
        truncated: false,
      };
    });
    const first = useWorkbenchStore.getState().loadDirectory(target);
    await vi.waitFor(() => expect(calls).toBe(1));
    useWorkbenchStore.setState({ directoryListings: {} });
    await useWorkbenchStore.getState().loadDirectory(target);
    old.resolve({ resource: target, entries: [], truncated: false });
    await first;
    const listing =
      useWorkbenchStore.getState().directoryListings[resourceKey(target)];
    expect(listing).toEqual(
      expect.objectContaining({
        phase: "ready",
        value: expect.objectContaining({
          entries: [expect.objectContaining({ name: "new.txt" })],
        }),
      }),
    );
  });

  it("bounds every ancestor inserted by Explorer reveal", async () => {
    vi.spyOn(desktop, "listDirectory").mockImplementation(async (target) => ({
      resource: target,
      entries: [],
      truncated: false,
    }));
    const path = `${Array.from({ length: 80 }, (_, index) => `d${index}`).join("/")}/file.txt`;
    await useWorkbenchStore.getState().revealFile(resource(path));
    expect(
      Object.keys(useWorkbenchStore.getState().expandedDirectories),
    ).toHaveLength(64);
  });

  it.each(["editor", "diff"] as const)(
    "keeps a newer same-resource %s selection when the older request finishes last",
    async (kind) => {
      const target = resource("aba.txt");
      const tab: WorkbenchTab = {
        id: `${kind}:${resourceKey(target)}`,
        kind: kind === "editor" ? "file" : "diff",
        title: "aba.txt",
        position: 1,
        pinned: false,
        dirty: false,
        resource: target,
      };
      useWorkbenchStore.setState({ tabs: [chatTab, tab], activeTabId: tab.id });
      if (kind === "editor") {
        const old = deferred<EditorDocument>();
        let calls = 0;
        vi.spyOn(desktop, "openEditorDocument").mockImplementation(() => {
          calls += 1;
          return calls === 1
            ? old.promise
            : Promise.resolve(document(target, "new", 2, false));
        });
        const first = useWorkbenchStore.getState().loadEditor(target);
        await vi.waitFor(() => expect(calls).toBe(1));
        await useWorkbenchStore.getState().activateTab(chatTab.id);
        await useWorkbenchStore.getState().activateTab(tab.id);
        old.resolve(document(target, "old", 1, false));
        await first;
        const editor = useWorkbenchStore.getState().activeEditor;
        expect(editor?.phase).toBe("ready");
        if (editor?.phase === "ready") expect(editor.content).toBe("new");
      } else {
        const old = deferred<FileDiff>();
        let calls = 0;
        vi.spyOn(desktop, "getFileDiff").mockImplementation(() => {
          calls += 1;
          return calls === 1
            ? old.promise
            : Promise.resolve({
                resource: target,
                hunks: [{ header: "new", lines: [] }],
                binary: false,
                truncated: false,
              });
        });
        const first = useWorkbenchStore.getState().loadDiff(target);
        await vi.waitFor(() => expect(calls).toBe(1));
        await useWorkbenchStore.getState().activateTab(chatTab.id);
        await useWorkbenchStore.getState().activateTab(tab.id);
        await vi.waitFor(() =>
          expect(
            useWorkbenchStore.getState().diffs[resourceKey(target)],
          ).toEqual(expect.objectContaining({ phase: "ready" })),
        );
        old.resolve({
          resource: target,
          hunks: [],
          binary: false,
          truncated: false,
        });
        await first;
        const diff = useWorkbenchStore.getState().diffs[resourceKey(target)];
        expect(diff).toEqual(
          expect.objectContaining({
            phase: "ready",
            value: expect.objectContaining({
              hunks: [expect.objectContaining({ header: "new" })],
            }),
          }),
        );
      }
    },
  );

  it("prevents harness ABA completions and clears pending prompt state on switch", async () => {
    const firstOld = deferred<ModelOption[]>();
    let oldCalls = 0;
    vi.spyOn(desktop, "listHarnessModels").mockImplementation((harnessId) => {
      if (harnessId === "old" && oldCalls++ === 0) return firstOld.promise;
      return Promise.resolve([{ id: `${harnessId}-latest`, name: "Latest" }]);
    });
    const first = useWorkbenchStore.getState().selectHarness("old");
    await useWorkbenchStore.getState().selectHarness("new");
    await useWorkbenchStore.getState().selectHarness("old");
    firstOld.resolve([{ id: "old-stale", name: "Stale" }]);
    await first;
    expect(useWorkbenchStore.getState().models).toEqual([
      { id: "old-latest", name: "Latest" },
    ]);

    useWorkbenchStore.setState({ sending: true, composerText: "old draft" });
    vi.spyOn(desktop, "switchProject").mockResolvedValue({
      committed: true,
      transitionEpoch: 1,
      snapshot: snapshotB(),
      recoveryError: null,
    });
    vi.spyOn(desktop, "listDirectory").mockResolvedValue({
      resource: resource("", "root-b"),
      entries: [],
      truncated: false,
    });
    await useWorkbenchStore.getState().switchProject("project-b");
    expect(useWorkbenchStore.getState().sending).toBe(false);
    expect(useWorkbenchStore.getState().composerText).toBe("");
    expect(useWorkbenchStore.getState().models).toEqual([]);
  });
});
