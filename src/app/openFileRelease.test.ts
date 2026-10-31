import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  RenameProjectFileResult,
  TrashProjectResourceResult,
  WorkbenchTab,
} from "../domain/workbench";
import { explorerSelectionAction } from "../features/files/explorerActionDialogController";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { UNSAVED_FILE_REASON } from "./openFileRelease";
import { useWorkbenchStore } from "./store";

const projectPort = getWorkbenchPorts().project;
const initialState = useWorkbenchStore.getState();
const source = { rootId: "root-a", relativePath: "src/old.ts" };
const target = { rootId: "root-a", relativePath: "src/new.ts" };
const parent = { rootId: "root-a", relativePath: "src" };

const chatTab: WorkbenchTab = {
  id: "chat-a",
  kind: "chat",
  title: "Chat",
  position: 0,
  pinned: false,
  dirty: false,
};

function fileTab(
  kind: "file" | "diff",
  position: number,
  dirty = false,
): WorkbenchTab {
  return {
    id: `${kind}:${source.relativePath}`,
    kind,
    title: "old.ts",
    position,
    pinned: false,
    dirty,
    resource: source,
  };
}

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
    tabs: [chatTab],
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

function renamed(): RenameProjectFileResult {
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
    snapshot: snapshot(target),
    warning: null,
  };
}

function trashed(): TrashProjectResourceResult {
  return {
    outcome: "committed",
    resourceConfirmed: true,
    projectId: "project-a",
    projectGeneration: 3,
    transitionEpoch: 6,
    source,
    parent,
    parentListing: { resource: parent, entries: [], truncated: false },
    snapshot: snapshot(parent),
    warning: null,
  };
}

const parentListing = {
  phase: "ready" as const,
  value: {
    resource: parent,
    truncated: false,
    entries: [
      { name: "old.ts", resource: source, kind: "file" as const, byteLen: 9 },
    ],
  },
};

beforeEach(() => {
  vi.spyOn(projectPort, "persistProjectLayout").mockResolvedValue();
  useWorkbenchStore.setState({
    ...snapshot(source),
    phase: "ready",
    projectGeneration: 3,
    nativeTransitionEpoch: 5,
    projectTransitioning: false,
    directoryListings: { [resourceKey(parent)]: parentListing },
    fileRenameInFlight: false,
    fileRenameError: null,
    resourceTrashInFlight: false,
    resourceTrashError: null,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

const state = () => useWorkbenchStore.getState();
const resourceTabs = () =>
  state().tabs.filter((tab) => tab.kind === "file" || tab.kind === "diff");

describe("renaming an open File (ED-16)", () => {
  it("closes its tabs for the rename and reopens the File tab where it stood", async () => {
    const rename = vi
      .spyOn(projectPort, "renameProjectFile")
      .mockResolvedValue(renamed());
    useWorkbenchStore.setState({
      tabs: [
        chatTab,
        { ...fileTab("file", 4), pinned: true },
        fileTab("diff", 5),
      ],
    });

    await expect(
      state().renameProjectFile(source, "new.ts", true),
    ).resolves.toBe(true);

    // Native saw a layout without the File's tabs.
    const request = rename.mock.calls[0][0];
    expect(
      request.currentLayout.tabs.some((tab) => tab.resource !== undefined),
    ).toBe(false);
    // The File tab is back, renamed, at its old place; the Diff tab is not.
    expect(resourceTabs()).toEqual([
      expect.objectContaining({
        kind: "file",
        resource: target,
        position: 4,
        pinned: true,
      }),
    ]);
    expect(state().activeTabId).toBe("chat-a");
  });

  it("reopens the renamed File as the active tab when it was active", async () => {
    vi.spyOn(projectPort, "renameProjectFile").mockResolvedValue(renamed());
    const openFile = vi.fn(async () => {});
    useWorkbenchStore.setState({
      tabs: [chatTab, fileTab("file", 1)],
      activeTabId: fileTab("file", 1).id,
      openFile,
    });

    const ok = await state().renameProjectFile(source, "new.ts", true);
    expect(ok).toBe(true);
    expect(openFile).toHaveBeenCalledWith(target);
  });

  it("refuses a File with unsaved changes and leaves its tab alone", async () => {
    const rename = vi.spyOn(projectPort, "renameProjectFile");
    const tabs = [chatTab, fileTab("file", 1, true)];
    useWorkbenchStore.setState({ tabs });

    await expect(
      state().renameProjectFile(source, "new.ts", true),
    ).resolves.toBe(false);
    expect(state().fileRenameError).toBe(UNSAVED_FILE_REASON);
    expect(state().tabs).toBe(tabs);
    expect(rename).not.toHaveBeenCalled();
  });

  it("puts the File back when the rename fails", async () => {
    vi.spyOn(projectPort, "renameProjectFile").mockRejectedValue(
      new Error("Target already exists."),
    );
    useWorkbenchStore.setState({ tabs: [chatTab, fileTab("file", 2)] });

    await expect(
      state().renameProjectFile(source, "new.ts", true),
    ).resolves.toBe(false);
    expect(state().fileRenameError).toContain("Target already exists.");
    expect(resourceTabs()).toEqual([
      expect.objectContaining({ kind: "file", resource: source, position: 2 }),
    ]);
    expect(state().selectedResource).toEqual(source);
  });

  it("still requires the tab closed when the caller doesn't release it", async () => {
    const rename = vi.spyOn(projectPort, "renameProjectFile");
    useWorkbenchStore.setState({ tabs: [chatTab, fileTab("file", 1)] });

    await expect(state().renameProjectFile(source, "new.ts")).resolves.toBe(
      false,
    );
    expect(state().fileRenameError).toContain("Close File and Diff tabs");
    expect(rename).not.toHaveBeenCalled();
  });
});

describe("recycling an open File (ED-16)", () => {
  it("closes its tabs and moves it", async () => {
    const trash = vi
      .spyOn(projectPort, "trashProjectResource")
      .mockResolvedValue(trashed());
    useWorkbenchStore.setState({ tabs: [chatTab, fileTab("file", 1)] });

    await expect(
      state().trashProjectResource(source, "file", true),
    ).resolves.toBe(true);
    expect(trash).toHaveBeenCalledOnce();
    expect(resourceTabs()).toEqual([]);
  });

  it("puts the File back when the move fails", async () => {
    vi.spyOn(projectPort, "trashProjectResource").mockRejectedValue(
      new Error("Trash is unavailable."),
    );
    useWorkbenchStore.setState({ tabs: [chatTab, fileTab("file", 3)] });

    await expect(
      state().trashProjectResource(source, "file", true),
    ).resolves.toBe(false);
    expect(resourceTabs()).toEqual([
      expect.objectContaining({ kind: "file", resource: source, position: 3 }),
    ]);
  });

  it("refuses a File with unsaved changes", async () => {
    const trash = vi.spyOn(projectPort, "trashProjectResource");
    useWorkbenchStore.setState({ tabs: [chatTab, fileTab("file", 1, true)] });

    await expect(
      state().trashProjectResource(source, "file", true),
    ).resolves.toBe(false);
    expect(state().resourceTrashError).toBe(UNSAVED_FILE_REASON);
    expect(trash).not.toHaveBeenCalled();
  });
});

describe("Explorer's actions for an open File", () => {
  const selection = (dirty: boolean) => ({
    activeEditor: null,
    directoryListings: { [resourceKey(parent)]: parentListing },
    selectedResource: source,
    tabs: [chatTab, fileTab("file", 1, dirty)],
  });

  it("offers rename and recycle in the Editor unless it has unsaved changes", () => {
    expect(explorerSelectionAction(selection(false), true)).toEqual({
      kind: "file",
      source,
    });
    expect(explorerSelectionAction(selection(true), true)).toEqual({
      kind: "unavailable",
      reason: UNSAVED_FILE_REASON,
    });
  });

  it("asks to close it first elsewhere", () => {
    expect(explorerSelectionAction(selection(false))).toEqual({
      kind: "unavailable",
      reason: "Close this resource before renaming or recycling it.",
    });
  });
});
