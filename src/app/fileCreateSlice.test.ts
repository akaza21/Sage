import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  CreateProjectFileResult,
  EditorDocument,
  ResourceRef,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const projectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();
const oldResource = { rootId: "root-a", relativePath: "src/old.ts" };
const parent = { rootId: "root-a", relativePath: "src" };
const created = { rootId: "root-a", relativePath: "src/new.ts" };

function document(resource: ResourceRef, content = ""): EditorDocument {
  const version = {
    fingerprint: `disk-${resource.relativePath}`,
    byteLen: 0,
    modifiedAt: null,
  };
  return {
    resource,
    disk: version,
    base: version,
    buffer: { ...version, revision: 0, content },
    lineEnding: "none",
    dirty: false,
    conflict: null,
  };
}

function result(): CreateProjectFileResult {
  return {
    outcome: "committed",
    resourceConfirmed: true,
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 5,
    resource: created,
    parentListing: {
      resource: parent,
      truncated: false,
      entries: [
        { name: "new.ts", resource: created, kind: "file", byteLen: 0 },
      ],
    },
    warning: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

beforeEach(() => {
  useWorkbenchStore.setState({
    phase: "ready",
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
    projectGeneration: 3,
    nativeTransitionEpoch: 5,
    projectTransitioning: false,
    tabs: [
      {
        id: "old",
        kind: "file",
        title: "old.ts",
        position: 0,
        pinned: false,
        dirty: true,
        resource: oldResource,
      },
    ],
    activeTabId: "old",
    selectedRootId: "root-a",
    selectedResource: oldResource,
    directoryListings: {},
    expandedDirectories: {},
    activeEditor: null,
    fileCreateInFlight: false,
    fileCreateError: null,
    fileCreateRequestGeneration: 0,
  });
  vi.spyOn(desktop, "openEditorDocument").mockResolvedValue(document(created));
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("file creation store transaction", () => {
  it("flushes and preserves a dirty editor, refreshes the exact parent, and opens the file", async () => {
    const oldDocument = document(oldResource, "dirty");
    useWorkbenchStore.setState({
      activeEditor: {
        phase: "ready",
        resource: oldResource,
        document: { ...oldDocument, dirty: true },
        content: "dirty change",
        revision: 2,
        persistedRevision: 1,
        persisting: false,
        saving: false,
        message: null,
      },
    });
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
      .mockResolvedValue({
        revision: 2,
        persistedAt: 1,
        dirty: true,
      });
    const create = vi
      .spyOn(projectPort, "createProjectFile")
      .mockResolvedValue(result());

    expect(
      await useWorkbenchStore.getState().createProjectFile(parent, "new.ts"),
    ).toBe(true);

    expect(snapshot).toHaveBeenCalledOnce();
    expect(snapshot.mock.invocationCallOrder[0]).toBeLessThan(
      create.mock.invocationCallOrder[0],
    );
    expect(
      useWorkbenchStore.getState().tabs.find((tab) => tab.id === "old")?.dirty,
    ).toBe(true);
    expect(useWorkbenchStore.getState().tabs.at(-1)).toMatchObject({
      kind: "file",
      resource: created,
    });
    expect(useWorkbenchStore.getState().activeTabId).toContain("src/new.ts");
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toEqual({
      phase: "ready",
      value: result().parentListing,
    });
    expect(useWorkbenchStore.getState().fileCreateNotice).toBeNull();
  });

  it("applies committed disk truth and opens the exact file when durability or refresh warns", async () => {
    vi.spyOn(projectPort, "createProjectFile").mockResolvedValue({
      ...result(),
      outcome: "effectUnknown",
      warning:
        "The file exists, but durability could not be confirmed; Explorer used a reconciled projection.",
    });

    expect(
      await useWorkbenchStore.getState().createProjectFile(parent, "new.ts"),
    ).toBe(true);

    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: expect.stringContaining("src/new.ts"),
      fileCreateError: null,
      fileCreateNotice:
        "The file exists, but durability could not be confirmed; Explorer used a reconciled projection.",
    });
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toEqual({ phase: "ready", value: result().parentListing });
    expect(desktop.openEditorDocument).toHaveBeenCalledWith(created);
  });

  it("refreshes and warns without opening an unconfirmed effect", async () => {
    const loadDirectory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      loadDirectory,
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: { resource: parent, entries: [], truncated: false },
        },
      },
    });
    vi.spyOn(projectPort, "createProjectFile").mockResolvedValue({
      ...result(),
      outcome: "effectUnknown",
      resourceConfirmed: false,
      parentListing: { ...result().parentListing, entries: [] },
      warning: "The file creation outcome is unknown; Explorer is refreshing.",
    });

    expect(
      await useWorkbenchStore.getState().createProjectFile(parent, "new.ts"),
    ).toBe(true);

    expect(loadDirectory).toHaveBeenCalledWith(parent);
    expect(desktop.openEditorDocument).not.toHaveBeenCalled();
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toBeUndefined();
    expect(useWorkbenchStore.getState()).toMatchObject({
      activeTabId: "old",
      fileCreateNotice:
        "The file creation outcome is unknown; Explorer is refreshing.",
    });
  });

  it("preserves tree, tabs, and dirty editor when native creation fails", async () => {
    const activeEditor = {
      phase: "ready" as const,
      resource: oldResource,
      document: { ...document(oldResource, "dirty"), dirty: true },
      content: "dirty",
      revision: 1,
      persistedRevision: 1,
      persisting: false,
      saving: false,
      message: null,
    };
    const listing = {
      [resourceKey(parent)]: {
        phase: "ready" as const,
        value: { resource: parent, entries: [], truncated: false },
      },
    };
    useWorkbenchStore.setState({ activeEditor, directoryListings: listing });
    vi.spyOn(projectPort, "createProjectFile").mockRejectedValue(
      new Error("disk denied"),
    );

    expect(
      await useWorkbenchStore.getState().createProjectFile(parent, "new.ts"),
    ).toBe(false);
    expect(useWorkbenchStore.getState()).toMatchObject({
      tabs: [expect.objectContaining({ id: "old", dirty: true })],
      directoryListings: listing,
      activeEditor,
      fileCreateError: "disk denied",
    });
  });

  it("drops a completion after a Project generation switch", async () => {
    const pending = deferred<CreateProjectFileResult>();
    vi.spyOn(projectPort, "createProjectFile").mockReturnValue(pending.promise);
    const create = useWorkbenchStore
      .getState()
      .createProjectFile(parent, "new.ts");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().fileCreateInFlight).toBe(true),
    );
    useWorkbenchStore.setState({
      projectGeneration: 4,
      nativeTransitionEpoch: 6,
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
      tabs: [],
      directoryListings: {},
    });
    pending.resolve(result());
    expect(await create).toBe(false);
    expect(useWorkbenchStore.getState().tabs).toEqual([]);
    expect(useWorkbenchStore.getState().directoryListings).toEqual({});
  });
});
