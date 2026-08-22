import { readFileSync } from "node:fs";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  RenameProjectFolderResult,
} from "../domain/workbench";
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

  it("keeps Explorer selector-only and gives truncated listings a Project-port refresh path", () => {
    const explorerFiles = ["ExplorerSurface.tsx", "ExplorerActions.tsx"];
    for (const file of explorerFiles) {
      const source = readFileSync(
        path.resolve(import.meta.dirname, `../features/files/${file}`),
        "utf8",
      );
      expect(source).not.toMatch(
        /from\s+["'][^"']*(?:desktop|workbenchRuntime)[^"']*["']/,
      );
    }
    const resourceSlice = readFileSync(
      path.resolve(import.meta.dirname, "./resourceSlice.ts"),
      "utf8",
    );
    expect(resourceSlice).toMatch(
      /getWorkbenchPorts\(\)\.project\.listDirectory/,
    );
  });
});
