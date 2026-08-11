import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  TrashProjectResourceResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { useWorkbenchStore } from "./store";

const initialState = useWorkbenchStore.getState();
const source = { rootId: "root-b", relativePath: "src/old" };
const child = { rootId: "root-b", relativePath: "src/old/child.ts" };
const siblingPrefix = { rootId: "root-b", relativePath: "src/older" };
const otherRoot = { rootId: "root-a", relativePath: "src/old/child.ts" };
const parent = { rootId: "root-b", relativePath: "src" };

function snapshot(selectedResource = parent): BootstrapSnapshot {
  return {
    transitionEpoch: 6,
    projects: [{ id: "project-a", name: "A", rootCount: 2, lastOpenedAt: 1 }],
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
        {
          id: "root-b",
          name: "Beta",
          ordinal: 1,
          availability: "available",
          message: null,
        },
      ],
    },
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    selectedRootId: selectedResource.rootId,
    selectedResource,
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

function result(): TrashProjectResourceResult {
  return {
    outcome: "committed",
    resourceConfirmed: true,
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 6,
    source,
    parent,
    parentListing: { resource: parent, entries: [], truncated: false },
    snapshot: snapshot(),
    warning: null,
  };
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
            { name: "old", resource: source, kind: "directory", byteLen: null },
          ],
        },
      },
      [resourceKey(source)]: {
        phase: "ready",
        value: { resource: source, entries: [], truncated: false },
      },
      [resourceKey(child)]: { phase: "loading" },
      [resourceKey(siblingPrefix)]: {
        phase: "ready",
        value: { resource: siblingPrefix, entries: [], truncated: false },
      },
      [resourceKey(otherRoot)]: {
        phase: "ready",
        value: { resource: otherRoot, entries: [], truncated: false },
      },
    },
    expandedDirectories: {
      [resourceKey(source)]: true,
      [resourceKey(child)]: true,
      [resourceKey(siblingPrefix)]: true,
      [resourceKey(otherRoot)]: true,
    },
    diffs: {
      [resourceKey(child)]: { phase: "loading" },
      [resourceKey(siblingPrefix)]: { phase: "loading" },
      [resourceKey(otherRoot)]: { phase: "loading" },
    },
    resourceTrashInFlight: false,
    resourceTrashError: null,
    resourceTrashNotice: null,
    resourceTrashRequestGeneration: 0,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("resource trash store transaction", () => {
  it("sends exact multi-root intent and purges only the deleted hierarchy", async () => {
    const trash = vi
      .spyOn(desktop, "trashProjectResource")
      .mockResolvedValue(result());

    await expect(
      useWorkbenchStore.getState().trashProjectResource(source, "emptyFolder"),
    ).resolves.toBe(true);

    expect(trash).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-a",
        projectGeneration: 3,
        expectedTransitionEpoch: 5,
        source,
        kind: "emptyFolder",
        currentLayout: expect.objectContaining({
          selectedRootId: "root-b",
          selectedResource: source,
        }),
      }),
    );
    const state = useWorkbenchStore.getState();
    expect(state.selectedResource).toEqual(parent);
    expect(state.projectGeneration).toBe(4);
    expect(state.nativeTransitionEpoch).toBe(6);
    expect(state.directoryListings[resourceKey(source)]).toBeUndefined();
    expect(state.directoryListings[resourceKey(child)]).toBeUndefined();
    expect(state.diffs[resourceKey(child)]).toBeUndefined();
    expect(state.directoryListings[resourceKey(siblingPrefix)]).toBeDefined();
    expect(state.directoryListings[resourceKey(otherRoot)]).toBeDefined();
  });

  it("leaves the projection untouched for typed unsupported", async () => {
    vi.spyOn(desktop, "trashProjectResource").mockResolvedValue({
      ...result(),
      outcome: "unsupported",
      resourceConfirmed: false,
      transitionEpoch: 5,
      parentListing: null,
      snapshot: null,
      warning: "Operating-system trash is unavailable.",
    });
    const beforeListings = useWorkbenchStore.getState().directoryListings;

    await expect(
      useWorkbenchStore.getState().trashProjectResource(source, "emptyFolder"),
    ).resolves.toBe(false);

    const state = useWorkbenchStore.getState();
    expect(state.projectGeneration).toBe(3);
    expect(state.nativeTransitionEpoch).toBe(5);
    expect(state.selectedResource).toEqual(source);
    expect(state.directoryListings).toBe(beforeListings);
    expect(state.resourceTrashError).toBe(
      "Operating-system trash is unavailable.",
    );
  });

  it("refreshes the exact parent and repositories after an unconfirmed effect", async () => {
    const loadDirectory = vi.fn(async () => undefined);
    const refreshRepository = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ loadDirectory, refreshRepository });
    vi.spyOn(desktop, "trashProjectResource").mockResolvedValue({
      ...result(),
      outcome: "effectUnknown",
      resourceConfirmed: false,
      parentListing: null,
      snapshot: snapshot(source),
      warning: "Recycle Bin outcome is unknown; Explorer is refreshing.",
    });

    await expect(
      useWorkbenchStore.getState().trashProjectResource(source, "emptyFolder"),
    ).resolves.toBe(true);

    expect(loadDirectory).toHaveBeenCalledWith(parent);
    expect(refreshRepository).toHaveBeenCalledOnce();
    expect(useWorkbenchStore.getState().resourceTrashNotice).toContain(
      "outcome is unknown",
    );
  });

  it("blocks an open descendant Diff before native mutation", async () => {
    const trash = vi.spyOn(desktop, "trashProjectResource");
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "descendant",
          kind: "diff",
          title: "child.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: child,
        },
      ],
    });

    await expect(
      useWorkbenchStore.getState().trashProjectResource(source, "emptyFolder"),
    ).resolves.toBe(false);

    expect(trash).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().resourceTrashError).toContain(
      "Close File and Diff tabs",
    );
  });
});
