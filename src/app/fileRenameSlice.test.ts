import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  RenameProjectFileResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const projectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();
const source = { rootId: "root-a", relativePath: "src/old.ts" };
const target = { rootId: "root-a", relativePath: "src/new.ts" };
const parent = { rootId: "root-a", relativePath: "src" };

function snapshot(selected = target): BootstrapSnapshot {
  return {
    transitionEpoch: 6,
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
    tabs: [
      {
        id: "chat-a",
        kind: "chat",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
    archivedChats: [],
    activeTabId: "chat-a",
    selectedRootId: "root-a",
    selectedResource: selected,
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

function result(): RenameProjectFileResult {
  return {
    outcome: "committed",
    resourceConfirmed: true,
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 6,
    source,
    target,
    parentListing: {
      resource: parent,
      truncated: false,
      entries: [{ name: "new.ts", resource: target, kind: "file", byteLen: 9 }],
    },
    snapshot: snapshot(),
    warning: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.spyOn(projectPort, "persistProjectLayout").mockResolvedValue();
  useWorkbenchStore.setState({
    ...snapshot(source),
    phase: "ready",
    projectGeneration: 3,
    nativeTransitionEpoch: 5,
    projectTransitioning: false,
    directoryListings: {
      [resourceKey(parent)]: {
        phase: "ready",
        value: {
          resource: parent,
          truncated: false,
          entries: [
            { name: "old.ts", resource: source, kind: "file", byteLen: 9 },
          ],
        },
      },
    },
    fileRenameInFlight: false,
    fileRenameError: null,
    fileRenameNotice: null,
    fileRenameRequestGeneration: 0,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("file rename store transaction", () => {
  it("sends the closed-resource layout and adopts snapshot plus exact parent", async () => {
    const rename = vi
      .spyOn(projectPort, "renameProjectFile")
      .mockResolvedValue(result());

    await expect(
      useWorkbenchStore.getState().renameProjectFile(source, "new.ts"),
    ).resolves.toBe(true);

    expect(rename).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-a",
        projectGeneration: 3,
        expectedTransitionEpoch: 5,
        source,
        basename: "new.ts",
        currentLayout: expect.objectContaining({
          tabs: [expect.objectContaining({ id: "chat-a" })],
          selectedResource: source,
        }),
      }),
    );
    expect(projectPort.persistProjectLayout).toHaveBeenCalledWith(
      expect.objectContaining({
        tabs: [expect.objectContaining({ id: "chat-a" })],
        selectedResource: source,
      }),
      5,
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      selectedResource: target,
      nativeTransitionEpoch: 6,
      projectGeneration: 4,
      projectTransitioning: false,
      fileRenameInFlight: false,
      fileRenameError: null,
      fileRenameNotice: null,
    });
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toEqual({ phase: "ready", value: result().parentListing });
  });

  it("preserves unrelated live UI and nested Explorer state", async () => {
    const root = { rootId: "root-a", relativePath: "" };
    const nested = { rootId: "root-a", relativePath: "other/nested" };
    const unrelatedDiff = { rootId: "root-a", relativePath: "other/change.ts" };
    const rootListing = {
      resource: root,
      truncated: false,
      entries: [
        {
          name: "src",
          resource: parent,
          kind: "directory" as const,
          byteLen: null,
        },
      ],
    };
    const nestedListing = {
      resource: nested,
      truncated: false,
      entries: [],
    };
    const diffState = {
      phase: "ready" as const,
      value: {
        resource: unrelatedDiff,
        hunks: [{ header: "@@ preserved @@", lines: [] }],
        binary: false,
        truncated: false,
      },
    };
    useWorkbenchStore.setState((state) => ({
      composerText: "unsent draft",
      models: [{ id: "model-live", name: "Live model" }],
      selectedModelId: "model-live",
      directoryListings: {
        ...state.directoryListings,
        [resourceKey(root)]: { phase: "ready", value: rootListing },
        [resourceKey(nested)]: { phase: "ready", value: nestedListing },
      },
      expandedDirectories: {
        [resourceKey(root)]: true,
        [resourceKey(parent)]: true,
        [resourceKey(nested)]: true,
      },
      diffs: { [resourceKey(unrelatedDiff)]: diffState },
    }));
    vi.spyOn(projectPort, "renameProjectFile").mockResolvedValue(result());

    await expect(
      useWorkbenchStore.getState().renameProjectFile(source, "new.ts"),
    ).resolves.toBe(true);

    const state = useWorkbenchStore.getState();
    expect(state.composerText).toBe("unsent draft");
    expect(state.models).toEqual([{ id: "model-live", name: "Live model" }]);
    expect(state.selectedModelId).toBe("model-live");
    expect(state.directoryListings[resourceKey(root)]).toEqual({
      phase: "ready",
      value: rootListing,
    });
    expect(state.directoryListings[resourceKey(nested)]).toEqual({
      phase: "ready",
      value: nestedListing,
    });
    expect(state.expandedDirectories[resourceKey(nested)]).toBe(true);
    expect(state.diffs[resourceKey(unrelatedDiff)]).toEqual(diffState);
  });

  it("rejects an open source or target File/Diff tab before native mutation", async () => {
    const rename = vi.spyOn(projectPort, "renameProjectFile");
    for (const [kind, resource] of [
      ["file", source],
      ["diff", source],
      ["file", target],
      ["diff", target],
    ] as const) {
      useWorkbenchStore.setState({
        tabs: [
          snapshot(source).tabs[0],
          {
            id: `${kind}:${resource.relativePath}`,
            kind,
            title: resource.relativePath,
            position: 1,
            pinned: false,
            dirty: false,
            resource,
          },
        ],
      });
      await expect(
        useWorkbenchStore.getState().renameProjectFile(source, "new.ts"),
      ).resolves.toBe(false);
      expect(useWorkbenchStore.getState().fileRenameError).toContain(
        "Close File and Diff tabs",
      );
    }
    expect(rename).not.toHaveBeenCalled();
  });

  it("retires a stale response without replacing the newer Project", async () => {
    const pending = deferred<RenameProjectFileResult>();
    vi.spyOn(projectPort, "renameProjectFile").mockReturnValue(pending.promise);
    const rename = useWorkbenchStore
      .getState()
      .renameProjectFile(source, "new.ts");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().fileRenameInFlight).toBe(true),
    );
    const newerListings = {};
    useWorkbenchStore.setState({
      projectGeneration: 4,
      nativeTransitionEpoch: 6,
      directoryListings: newerListings,
      fileRenameInFlight: false,
    });
    pending.resolve(result());

    await expect(rename).resolves.toBe(false);
    expect(useWorkbenchStore.getState().directoryListings).toBe(newerListings);
    expect(useWorkbenchStore.getState().selectedResource).toEqual(source);
  });

  it("owns shared Chat and window layout admission while persistence is deferred", async () => {
    const persistence = deferred<void>();
    vi.mocked(projectPort.persistProjectLayout).mockReturnValue(
      persistence.promise,
    );
    vi.spyOn(projectPort, "renameProjectFile").mockResolvedValue(result());
    const createChat = vi.spyOn(desktop, "createChat");
    const activateChat = vi.spyOn(desktop, "activateChat");
    const close = vi.spyOn(projectPort, "persistProjectLayoutAndClose");

    const rename = useWorkbenchStore
      .getState()
      .renameProjectFile(source, "new.ts");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState()).toMatchObject({
        fileRenameInFlight: true,
        projectTransitioning: true,
      }),
    );

    await useWorkbenchStore.getState().createChat();
    await useWorkbenchStore.getState().activateChat("chat-a");
    await expect(useWorkbenchStore.getState().closeWorkbench()).rejects.toThrow(
      "layout mutation",
    );
    expect(createChat).not.toHaveBeenCalled();
    expect(activateChat).not.toHaveBeenCalled();
    expect(close).not.toHaveBeenCalled();

    persistence.resolve();
    await expect(rename).resolves.toBe(true);
    expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
  });

  it("keeps an unconfirmed effect typed, visible, and refreshable", async () => {
    const loadDirectory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ loadDirectory });
    vi.spyOn(projectPort, "renameProjectFile").mockResolvedValue({
      ...result(),
      outcome: "effectUnknown",
      resourceConfirmed: false,
      parentListing: null,
      snapshot: snapshot(source),
      warning: "Rename outcome is unknown; Explorer was refreshed.",
    });

    await expect(
      useWorkbenchStore.getState().renameProjectFile(source, "new.ts"),
    ).resolves.toBe(true);

    expect(loadDirectory).toHaveBeenCalledWith(parent);
    expect(useWorkbenchStore.getState()).toMatchObject({
      selectedResource: source,
      fileRenameError: null,
      fileRenameNotice: "Rename outcome is unknown; Explorer was refreshed.",
    });
  });

  it("renames one selected empty folder through the folder Project port", async () => {
    const folder = { rootId: "root-a", relativePath: "src/empty" };
    const renamed = { rootId: "root-a", relativePath: "src/renamed" };
    useWorkbenchStore.setState({
      ...snapshot(folder),
      phase: "ready",
      projectGeneration: 3,
      nativeTransitionEpoch: 5,
      projectTransitioning: false,
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            truncated: false,
            entries: [
              {
                name: "empty",
                resource: folder,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(folder)]: {
          phase: "ready",
          value: { resource: folder, truncated: false, entries: [] },
        },
      },
    });
    const rename = vi
      .spyOn(projectPort, "renameProjectFolder")
      .mockResolvedValue({
        ...result(),
        source: folder,
        target: renamed,
        parentListing: {
          resource: parent,
          truncated: false,
          entries: [
            {
              name: "renamed",
              resource: renamed,
              kind: "directory",
              byteLen: null,
            },
          ],
        },
        snapshot: snapshot(renamed),
      });

    await expect(
      useWorkbenchStore.getState().renameProjectFolder(folder, "renamed"),
    ).resolves.toBe(true);

    expect(rename).toHaveBeenCalledWith(
      expect.objectContaining({ source: folder, basename: "renamed" }),
    );
    expect(useWorkbenchStore.getState().selectedResource).toEqual(renamed);
  });

  it("drops the renamed folder's stale cache and preserves its expanded state at the target", async () => {
    const folder = { rootId: "root-a", relativePath: "src/empty" };
    const renamed = { rootId: "root-a", relativePath: "src/renamed" };
    const staleDescendant = {
      rootId: "root-a",
      relativePath: "src/empty/stale",
    };
    useWorkbenchStore.setState({
      ...snapshot(folder),
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            truncated: false,
            entries: [
              {
                name: "empty",
                resource: folder,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(folder)]: {
          phase: "ready",
          value: { resource: folder, truncated: false, entries: [] },
        },
        [resourceKey(staleDescendant)]: {
          phase: "ready",
          value: { resource: staleDescendant, truncated: false, entries: [] },
        },
      },
      expandedDirectories: {
        [resourceKey(folder)]: true,
        [resourceKey(staleDescendant)]: true,
      },
    });
    vi.spyOn(projectPort, "renameProjectFolder").mockResolvedValue({
      ...result(),
      source: folder,
      target: renamed,
      parentListing: {
        resource: parent,
        truncated: false,
        entries: [
          {
            name: "renamed",
            resource: renamed,
            kind: "directory",
            byteLen: null,
          },
        ],
      },
      snapshot: snapshot(renamed),
    });

    await useWorkbenchStore.getState().renameProjectFolder(folder, "renamed");

    const state = useWorkbenchStore.getState();
    expect(state.directoryListings[resourceKey(folder)]).toBeUndefined();
    expect(
      state.directoryListings[resourceKey(staleDescendant)],
    ).toBeUndefined();
    expect(state.expandedDirectories[resourceKey(folder)]).toBeUndefined();
    expect(
      state.expandedDirectories[resourceKey(staleDescendant)],
    ).toBeUndefined();
    expect(state.expandedDirectories[resourceKey(renamed)]).toBe(true);
    expect(state.directoryListings[resourceKey(parent)]).toEqual({
      phase: "ready",
      value: expect.objectContaining({
        entries: [expect.objectContaining({ resource: renamed })],
      }),
    });
  });

  it("refreshes the parent after an unconfirmed folder rename", async () => {
    const folder = { rootId: "root-a", relativePath: "src/empty" };
    useWorkbenchStore.setState({
      ...snapshot(folder),
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            truncated: false,
            entries: [
              {
                name: "empty",
                resource: folder,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(folder)]: {
          phase: "ready",
          value: { resource: folder, truncated: false, entries: [] },
        },
      },
    });
    const loadDirectory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ loadDirectory });
    vi.spyOn(projectPort, "renameProjectFolder").mockResolvedValue({
      ...result(),
      source: folder,
      target: { rootId: "root-a", relativePath: "src/renamed" },
      outcome: "effectUnknown",
      resourceConfirmed: false,
      parentListing: null,
      snapshot: snapshot(folder),
      warning: "Folder rename outcome is unknown; Explorer was refreshed.",
    });

    await expect(
      useWorkbenchStore.getState().renameProjectFolder(folder, "renamed"),
    ).resolves.toBe(true);

    expect(loadDirectory).toHaveBeenCalledWith(parent);
    expect(useWorkbenchStore.getState().fileRenameNotice).toContain(
      "outcome is unknown",
    );
  });

  it("applies confirmed parent evidence when the snapshot is unavailable", async () => {
    const unrelated = { rootId: "root-a", relativePath: "other" };
    const unrelatedListing = {
      resource: unrelated,
      truncated: false,
      entries: [],
    };
    useWorkbenchStore.setState((state) => ({
      composerText: "preserve this draft",
      directoryListings: {
        ...state.directoryListings,
        [resourceKey(unrelated)]: {
          phase: "ready" as const,
          value: unrelatedListing,
        },
      },
    }));
    vi.spyOn(projectPort, "renameProjectFile").mockResolvedValue({
      ...result(),
      outcome: "effectUnknown",
      snapshot: null,
      warning: "The authoritative layout snapshot was unavailable.",
    });

    await expect(
      useWorkbenchStore.getState().renameProjectFile(source, "new.ts"),
    ).resolves.toBe(true);

    const state = useWorkbenchStore.getState();
    expect(state.selectedResource).toEqual(target);
    expect(state.directoryListings[resourceKey(parent)]).toEqual({
      phase: "ready",
      value: result().parentListing,
    });
    expect(state.directoryListings[resourceKey(unrelated)]).toEqual({
      phase: "ready",
      value: unrelatedListing,
    });
    expect(state.composerText).toBe("preserve this draft");
    expect(state.fileRenameNotice).toBe(
      "The authoritative layout snapshot was unavailable.",
    );
  });
});
