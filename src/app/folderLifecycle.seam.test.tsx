import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { childResource, resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  RenameProjectFolderResult,
} from "../domain/workbench";
import { ExplorerSurface } from "../features/files/ExplorerSurface";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useWorkbenchStore } from "./store";

const fakeProjectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();
const source = { rootId: "root-a", relativePath: "src/empty" };
const target = { rootId: "root-a", relativePath: "src/renamed" };
const parent = { rootId: "root-a", relativePath: "src" };

function snapshot(selected = target): BootstrapSnapshot {
  return {
    transitionEpoch: 6,
    projects: [],
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

function result(): RenameProjectFolderResult {
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
      entries: [
        {
          name: "renamed",
          resource: target,
          kind: "directory",
          byteLen: null,
        },
      ],
    },
    snapshot: snapshot(),
    warning: null,
  };
}

beforeEach(() => {
  vi.spyOn(fakeProjectPort, "persistProjectLayout").mockResolvedValue();
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
            {
              name: "empty",
              resource: source,
              kind: "directory",
              byteLen: null,
            },
          ],
        },
      },
      [resourceKey(source)]: {
        phase: "ready",
        value: { resource: source, truncated: false, entries: [] },
      },
    },
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

describe("folder lifecycle public seam", () => {
  it("renames an independently selected empty folder through the Project port", async () => {
    const renameProjectFolder = vi
      .spyOn(fakeProjectPort, "renameProjectFolder")
      .mockResolvedValue(result());

    await expect(
      useWorkbenchStore.getState().renameProjectFolder(source, "renamed"),
    ).resolves.toBe(true);

    expect(renameProjectFolder).toHaveBeenCalledWith(
      expect.objectContaining({ source, basename: "renamed" }),
    );
  });

  it("refreshes a truncated Explorer listing through the Project port and shows the new entries", async () => {
    const entry = (name: string) => ({
      name,
      resource: { rootId: "root-a", relativePath: `src/${name}` },
      kind: "file" as const,
      byteLen: 1,
    });
    const listDirectory = vi
      .spyOn(fakeProjectPort, "listDirectory")
      .mockResolvedValue({
        resource: parent,
        truncated: false,
        entries: [entry("first.ts"), entry("second.ts"), entry("third.ts")],
      });
    useWorkbenchStore.setState({
      project: snapshot().project,
      selectedResource: null,
      expandedDirectories: { [resourceKey(parent)]: true },
      directoryListings: {
        [resourceKey(childResource("root-a"))]: {
          phase: "ready",
          value: {
            resource: childResource("root-a"),
            truncated: false,
            entries: [
              {
                name: "src",
                resource: parent,
                kind: "directory",
                byteLen: null,
              },
            ],
          },
        },
        [resourceKey(parent)]: {
          phase: "ready",
          value: {
            resource: parent,
            truncated: true,
            entries: [entry("first.ts")],
          },
        },
      },
    });

    render(<ExplorerSurface />);
    expect(
      screen.getByText("More entries are hidden by the view limit."),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("treeitem", { name: "Open file src/third.ts" }),
    ).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Refresh folder" }));

    expect(
      await screen.findByRole("treeitem", { name: "Open file src/third.ts" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("treeitem", { name: "Open file src/second.ts" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("More entries are hidden by the view limit."),
    ).toBeNull();
    expect(listDirectory).toHaveBeenCalledWith(parent);
  });

  it("lets the current generation retry while an old directory listing is still hung", async () => {
    let resolveOldListing: (() => void) | undefined;
    const oldListing = new Promise<void>((resolve) => {
      resolveOldListing = resolve;
    });
    const listDirectory = vi
      .spyOn(fakeProjectPort, "listDirectory")
      .mockReturnValueOnce(
        oldListing.then(() => ({
          resource: parent,
          truncated: false,
          entries: [],
        })),
      )
      .mockResolvedValueOnce({
        resource: parent,
        truncated: false,
        entries: [],
      });

    const oldLoad = useWorkbenchStore.getState().loadDirectory(parent, true);
    await vi.waitFor(() =>
      expect(
        useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
      ).toEqual({ phase: "loading" }),
    );

    useWorkbenchStore.setState({ projectGeneration: 4 });
    await useWorkbenchStore.getState().loadDirectory(parent, true);

    expect(listDirectory).toHaveBeenCalledTimes(2);
    expect(
      useWorkbenchStore.getState().directoryListings[resourceKey(parent)],
    ).toEqual({
      phase: "ready",
      value: { resource: parent, truncated: false, entries: [] },
    });

    resolveOldListing?.();
    await oldLoad;
  });
});
