import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { resourceKey, sameResource } from "../domain/resourceIdentity";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import { applyEditorExternalWatchNotice } from "../features/files/editorExternalWatch";
import { RECREATE_BEFORE_LEAVING } from "../features/files/fileEditorController";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { browserEditor } from "../services/browserEditor";
import {
  browserDirectories,
  browserRef,
  browserSnapshot,
} from "../services/browserWorkspaceFixture";
import { CHANGED_ON_DISK } from "./editorConflictActions";
import { storeEditorBridge } from "./editorSlice";
import { useWorkbenchStore } from "./store";

// These run against the browser runtime's in-memory files, which keep the
// native rules: a buffer remembers its disk version and a save refuses once
// disk has moved on.
const editor = getWorkbenchPorts().editor;
const store = () => useWorkbenchStore.getState();
const bridge = storeEditorBridge(
  useWorkbenchStore.setState,
  useWorkbenchStore.getState,
);

let fileNumber = 0;
const nextFile = () => browserRef(`src/conflict-${(fileNumber += 1)}.ts`);

const fileTab = (resource: ResourceRef, position: number): WorkbenchTab => ({
  id: `file:${resourceKey(resource)}`,
  kind: "file",
  title: resource.relativePath,
  position,
  pinned: false,
  dirty: false,
  resource,
});

const tabOf = (resource: ResourceRef) =>
  store().tabs.find((tab) => sameResource(tab.resource, resource))!;

const diskText = async (resource: ResourceRef) =>
  (await editor.readTextFile(resource)).content;

function openTabs(...resources: ResourceRef[]) {
  useWorkbenchStore.setState({
    tabs: resources.map(fileTab),
    activeTabId: fileTab(resources[0], 0).id,
    activeEditor: null,
  });
}

/** Opens `resource` in the editor and types `content` over it. */
async function edit(resource: ResourceRef, content: string) {
  await store().loadEditor(resource);
  store().updateEditorContent(content);
}

/** A buffer left behind in a background tab, as switching away leaves one. */
async function backgroundBuffer(resource: ResourceRef, content: string) {
  const document = await editor.openEditorDocument(resource);
  await editor.snapshotEditorBuffer({
    resource,
    base: document.base,
    lineEnding: document.lineEnding,
    content,
    revision: 1,
  });
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.map((tab) =>
      sameResource(tab.resource, resource) ? { ...tab, dirty: true } : tab,
    ),
  }));
}

function changeOnDisk(resource: ResourceRef, content: string | null) {
  browserEditor.changeOnDisk(resource, content);
  return applyEditorExternalWatchNotice(
    { resource, generation: 1, kind: content === null ? "deleted" : "changed" },
    bridge,
    editor,
  );
}

function readyEditor() {
  const active = store().activeEditor;
  if (active?.phase !== "ready") throw new Error("no ready editor");
  return active;
}

beforeEach(() => {
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState({
    phase: "ready",
    project: browserSnapshot.project,
    projectGeneration: 1,
    nativeTransitionEpoch: browserSnapshot.transitionEpoch,
    projectTransitioning: false,
    saveAllReport: null,
  });
});

afterEach(() => {
  resetBrowserDesktopForTests();
});

describe("a File that changed on disk under unsaved edits", () => {
  it("Reload replaces the buffer with the File on disk", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    await changeOnDisk(file, "theirs\n");
    expect(readyEditor().document.conflict?.kind).toBe("external-change");
    expect(tabOf(file).status).toBe("failed");

    expect(await store().discardActiveEditor()).toBe(true);

    expect(readyEditor()).toMatchObject({ content: "theirs\n" });
    expect(readyEditor().document.conflict).toBeNull();
    expect(tabOf(file)).toMatchObject({ dirty: false, status: undefined });
    expect(await diskText(file)).toBe("theirs\n");
  });

  it("Keep mine lets the next save write the buffer over the change", async () => {
    const file = nextFile();
    openTabs(file);
    // Not yet snapshotted: the buffer exists only in the WebView.
    await edit(file, "mine\n");
    await changeOnDisk(file, "theirs\n");
    expect(await store().saveActiveEditor()).toBe(false);
    expect(await diskText(file)).toBe("theirs\n");

    expect(await store().keepActiveEditorBuffer()).toBe(true);
    expect(readyEditor().document.conflict).toBeNull();
    expect(readyEditor()).toMatchObject({ content: "mine\n" });
    expect(tabOf(file)).toMatchObject({ dirty: true, status: undefined });

    expect(await store().saveActiveEditor()).toBe(true);
    expect(await diskText(file)).toBe("mine\n");
    expect(tabOf(file).dirty).toBe(false);
  });

  it("Keep mine on a deleted File creates it again on save", async () => {
    const file = browserRef("src/workbench/Workbench.tsx");
    const listing = () =>
      browserDirectories
        .get(resourceKey(browserRef("src/workbench")))!
        .entries.map((entry) => entry.name);
    openTabs(file);
    await edit(file, "kept\n");
    await changeOnDisk(file, null);
    expect(readyEditor().document.conflict?.kind).toBe("deleted");
    expect(listing()).not.toContain("Workbench.tsx");

    expect(await store().keepActiveEditorBuffer()).toBe(true);
    expect(readyEditor()).toMatchObject({ recreateOnSave: true });
    // Leaving would lose the only copy, so it is refused until a save.
    await expect(store().flushActiveEditor()).rejects.toThrow(
      RECREATE_BEFORE_LEAVING,
    );

    expect(await store().saveActiveEditor()).toBe(true);
    expect(await diskText(file)).toBe("kept\n");
    expect(listing()).toContain("Workbench.tsx");
    expect(readyEditor().recreateOnSave).toBeUndefined();
    expect(tabOf(file).dirty).toBe(false);
  });

  it("shows the conflict when a background tab is activated", async () => {
    const [front, back] = [nextFile(), nextFile()];
    openTabs(front, back);
    await store().loadEditor(front);
    await backgroundBuffer(back, "mine\n");
    browserEditor.changeOnDisk(back, "theirs\n");

    await store().activateTab(tabOf(back).id);

    expect(readyEditor().resource).toEqual(back);
    expect(readyEditor()).toMatchObject({ content: "mine\n" });
    expect(readyEditor().document.conflict?.kind).toBe("external-change");
    expect(tabOf(back).status).toBe("failed");
  });
});

describe("Save All", () => {
  it("saves every dirty File and reports the one it could not", async () => {
    const [active, background, conflicted] = [
      nextFile(),
      nextFile(),
      nextFile(),
    ];
    openTabs(active, background, conflicted);
    await backgroundBuffer(background, "background\n");
    await backgroundBuffer(conflicted, "conflicted\n");
    browserEditor.changeOnDisk(conflicted, "changed elsewhere\n");
    await edit(active, "active\n");

    const failures = await store().saveAllEditors();

    expect(failures).toEqual([
      { resource: conflicted, message: CHANGED_ON_DISK },
    ]);
    expect(await diskText(active)).toBe("active\n");
    expect(await diskText(background)).toBe("background\n");
    expect(await diskText(conflicted)).toBe("changed elsewhere\n");
    expect(tabOf(active).dirty).toBe(false);
    expect(tabOf(background).dirty).toBe(false);
    expect(tabOf(conflicted)).toMatchObject({ dirty: true, status: "failed" });
    expect(store().saveAllReport).toEqual({
      projectId: browserSnapshot.project!.id,
      failures,
    });
  });
});
