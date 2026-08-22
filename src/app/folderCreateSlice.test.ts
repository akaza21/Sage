import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type { CreateProjectFolderResult } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const projectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();
const parent = { rootId: "root-b", relativePath: "src" };
const created = { rootId: "root-b", relativePath: "src/nested" };

function result(): CreateProjectFolderResult {
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
        { name: "nested", resource: created, kind: "directory", byteLen: null },
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
          id: "root-b",
          name: "B",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    },
    projectGeneration: 3,
    nativeTransitionEpoch: 5,
    projectTransitioning: false,
    selectedRootId: "root-b",
    selectedResource: parent,
    directoryListings: {},
    expandedDirectories: {},
    fileCreateInFlight: false,
    folderCreateInFlight: false,
    folderCreateError: null,
    folderCreateNotice: null,
    folderCreateRequestGeneration: 0,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("folder creation store transaction", () => {
  it("applies the exact root-qualified parent listing and selects the new folder", async () => {
    const loadDirectory = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ loadDirectory });
    vi.spyOn(projectPort, "createProjectFolder").mockResolvedValue(result());

    expect(
      await useWorkbenchStore.getState().createProjectFolder(parent, "nested"),
    ).toBe(true);

    expect(projectPort.createProjectFolder).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 5,
      parent,
      basename: "nested",
    });
    expect(useWorkbenchStore.getState().selectedResource).toEqual(created);
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toEqual({ phase: "ready", value: result().parentListing });
    expect(loadDirectory).toHaveBeenCalledWith(created);
  });

  it("refreshes only the exact parent when a committed effect cannot be confirmed", async () => {
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
    vi.spyOn(projectPort, "createProjectFolder").mockResolvedValue({
      ...result(),
      outcome: "effectUnknown",
      resourceConfirmed: false,
      parentListing: { ...result().parentListing, entries: [] },
      warning:
        "The folder creation outcome is unknown; Explorer is refreshing.",
    });

    expect(
      await useWorkbenchStore.getState().createProjectFolder(parent, "nested"),
    ).toBe(true);

    expect(loadDirectory).toHaveBeenCalledWith(parent);
    expect(useWorkbenchStore.getState().selectedResource).toEqual(parent);
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toBeUndefined();
    expect(useWorkbenchStore.getState().folderCreateNotice).toContain(
      "outcome is unknown",
    );
  });

  it("preserves tree selection and reports bounded native failure", async () => {
    useWorkbenchStore.setState({
      directoryListings: {
        [resourceKey(parent)]: {
          phase: "ready",
          value: { resource: parent, entries: [], truncated: false },
        },
      },
    });
    vi.spyOn(projectPort, "createProjectFolder").mockRejectedValue(
      new Error(`disk denied ${"x".repeat(3_000)}`),
    );

    expect(
      await useWorkbenchStore.getState().createProjectFolder(parent, "nested"),
    ).toBe(false);

    expect(useWorkbenchStore.getState().selectedResource).toEqual(parent);
    expect(useWorkbenchStore.getState().folderCreateError?.length).toBe(2_048);
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toBeDefined();
  });

  it("drops a completion after a Project generation switch", async () => {
    const pending = deferred<CreateProjectFolderResult>();
    vi.spyOn(projectPort, "createProjectFolder").mockReturnValue(
      pending.promise,
    );
    const create = useWorkbenchStore
      .getState()
      .createProjectFolder(parent, "nested");
    await vi.waitFor(() =>
      expect(useWorkbenchStore.getState().folderCreateInFlight).toBe(true),
    );
    useWorkbenchStore.setState({
      projectGeneration: 4,
      nativeTransitionEpoch: 6,
      project: null,
      selectedResource: null,
      directoryListings: {},
    });
    pending.resolve(result());

    expect(await create).toBe(false);
    expect(useWorkbenchStore.getState().selectedResource).toBeNull();
    expect(useWorkbenchStore.getState().directoryListings).toEqual({});
  });

  it("does not release an existing creation transaction on reentry", async () => {
    const create = vi.spyOn(projectPort, "createProjectFolder");
    useWorkbenchStore.setState({ folderCreateInFlight: true });

    expect(
      await useWorkbenchStore.getState().createProjectFolder(parent, "nested"),
    ).toBe(false);

    expect(useWorkbenchStore.getState().folderCreateInFlight).toBe(true);
    expect(create).not.toHaveBeenCalled();
  });
});
