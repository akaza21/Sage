import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  RenameProjectFileResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

const initialState = useWorkbenchStore.getState();
const source = { rootId: "root-a", relativePath: "src/old.ts" };
const target = { rootId: "root-a", relativePath: "src/new.ts" };
const parent = { rootId: "root-a", relativePath: "src" };

function snapshot(selected = target): BootstrapSnapshot {
  return {
    transitionEpoch: 5,
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
    transitionEpoch: 5,
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
  vi.spyOn(desktop, "persistProjectLayout").mockResolvedValue();
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
      .spyOn(desktop, "renameProjectFile")
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
    expect(desktop.persistProjectLayout).toHaveBeenCalledWith(
      expect.objectContaining({
        tabs: [expect.objectContaining({ id: "chat-a" })],
        selectedResource: source,
      }),
    );
    expect(useWorkbenchStore.getState()).toMatchObject({
      selectedResource: target,
      fileRenameInFlight: false,
      fileRenameError: null,
      fileRenameNotice: null,
    });
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toEqual({ phase: "ready", value: result().parentListing });
  });

  it("rejects an open source or target File/Diff tab before native mutation", async () => {
    const rename = vi.spyOn(desktop, "renameProjectFile");
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
    vi.spyOn(desktop, "renameProjectFile").mockReturnValue(pending.promise);
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

  it("keeps an unconfirmed effect typed, visible, and refreshable", async () => {
    const loadDirectory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ loadDirectory });
    vi.spyOn(desktop, "renameProjectFile").mockResolvedValue({
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
});
