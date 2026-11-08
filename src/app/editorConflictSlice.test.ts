import { cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { resourceKey, sameResource } from "../domain/resourceIdentity";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";
import FileConflictNotice from "../features/files/FileConflictNotice";
import { applyEditorExternalWatchNotice } from "../features/files/editorExternalWatch";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { browserEditor } from "../services/browserEditor";
import {
  browserDirectories,
  browserRef,
  browserSnapshot,
} from "../services/browserWorkspaceFixture";
import { CHANGED_ON_DISK, DELETED_ON_DISK } from "./editorConflictActions";
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

  it("keeps the newest edits when disk comes to match an older snapshot", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "first\n");
    await store().flushActiveEditor();
    store().updateEditorContent("second\n");
    // Another program writes what the durable buffer holds.
    await changeOnDisk(file, "first\n");

    expect(readyEditor()).toMatchObject({ content: "second\n" });
    // The recovery copy holds them too, should Sage stop now.
    expect((await editor.openEditorDocument(file)).buffer.content).toBe(
      "second\n",
    );
    await store().keepActiveEditorBuffer();
    expect(await store().saveActiveEditor()).toBe(true);
    expect(await diskText(file)).toBe("second\n");
  });

  it("says when unsaved edits could not be stored before the read", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    const snapshot = vi
      .spyOn(editor, "snapshotEditorBuffer")
      .mockRejectedValue(new Error("the disk is full"));
    try {
      await changeOnDisk(file, "theirs\n");
      expect(snapshot).toHaveBeenCalled();
    } finally {
      snapshot.mockRestore();
    }

    expect(readyEditor()).toMatchObject({ content: "mine\n" });
    expect(readyEditor().document.conflict?.kind).toBe("external-change");
    expect(readyEditor().message).toBe("Couldn't save: the disk is full.");
    expect(readyEditor().revision).toBeGreaterThan(
      readyEditor().persistedRevision,
    );
    render(
      createElement(FileConflictNotice, {
        document: readyEditor().document,
        editable: true,
      }),
    );
    expect(screen.getByRole("alert").textContent).toContain(
      "Your unsaved changes are not yet stored. Couldn't save: the disk is full.",
    );
    cleanup();
  });

  it("names a refused save plainly and says so when disk then changes", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    const save = vi
      .spyOn(editor, "saveEditorDocument")
      .mockRejectedValue(
        new Error(
          "filesystem operation failed: Access is denied. (os error 5)",
        ),
      );
    try {
      expect(await store().saveActiveEditor()).toBe(false);
    } finally {
      save.mockRestore();
    }
    const refused =
      "Couldn't save: the file is read-only or you don't have permission.";
    expect(readyEditor().message).toBe(refused);
    expect(store().error).toBe(refused);
    expect(tabOf(file).dirty).toBe(true);

    store().updateEditorContent("mine, more\n");
    await changeOnDisk(file, "theirs\n");

    expect(readyEditor().document.conflict?.kind).toBe("external-change");
    expect(readyEditor()).toMatchObject({ content: "mine, more\n" });
    render(
      createElement(FileConflictNotice, {
        document: readyEditor().document,
        editable: true,
      }),
    );
    expect(screen.getByRole("alert").textContent).toContain(
      `Your unsaved changes are not yet stored. ${refused}`,
    );
    cleanup();
    expect(await diskText(file)).toBe("theirs\n");
  });

  it("Keep mine is refused if disk changed again since the banner", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    await changeOnDisk(file, "first\n");
    // A second change the banner has not shown yet.
    browserEditor.changeOnDisk(file, "second\n");

    expect(await store().keepActiveEditorBuffer()).toBe(false);
    expect(readyEditor()).toMatchObject({ content: "mine\n", saving: false });
    expect(readyEditor().document.conflict?.kind).toBe("external-change");
    expect(tabOf(file).status).toBe("failed");
    expect(await diskText(file)).toBe("second\n");

    // The banner now shows the second change, and Keep mine goes on it.
    expect(await store().keepActiveEditorBuffer()).toBe(true);
    expect(await store().saveActiveEditor()).toBe(true);
    expect(await diskText(file)).toBe("mine\n");
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
    expect(readyEditor().document.conflict).toBeNull();
    // The kept buffer is durable, so the tab can be left and come back.
    store().updateEditorContent("kept, edited\n");
    await store().flushActiveEditor();
    useWorkbenchStore.setState({ activeEditor: null });
    await store().loadEditor(file);
    expect(readyEditor()).toMatchObject({ content: "kept, edited\n" });

    expect(await store().saveActiveEditor()).toBe(true);
    expect(await diskText(file)).toBe("kept, edited\n");
    expect(listing()).toContain("Workbench.tsx");
    expect(tabOf(file).dirty).toBe(false);
  });

  it.each([
    ["with unsaved edits", "mine\n"],
    ["with no edits", null],
  ])(
    "stays deleted when another notice follows the deletion, %s",
    async (_, edits) => {
      const file = nextFile();
      openTabs(file);
      await store().loadEditor(file);
      if (edits) store().updateEditorContent(edits);
      await changeOnDisk(file, null);
      expect(readyEditor().document.conflict?.kind).toBe("deleted");

      // Windows ends the watch of a deleted folder with a further notice.
      await applyEditorExternalWatchNotice(
        { resource: file, generation: 1, kind: "changed" },
        bridge,
        editor,
      );

      expect(readyEditor().document.conflict?.kind).toBe("deleted");
      expect(tabOf(file).status).toBe("failed");
      render(
        createElement(FileConflictNotice, {
          document: readyEditor().document,
          editable: true,
        }),
      );
      expect(screen.getByRole("alert").textContent).toContain("File deleted.");
      expect(screen.queryByRole("button", { name: "Reload" })).toBeNull();
      expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();
      cleanup();
    },
  );

  it("a kept deleted File stays kept when another notice follows", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "kept\n");
    await changeOnDisk(file, null);
    expect(await store().keepActiveEditorBuffer()).toBe(true);

    await applyEditorExternalWatchNotice(
      { resource: file, generation: 1, kind: "changed" },
      bridge,
      editor,
    );

    expect(readyEditor().document.conflict).toBeNull();
    expect(readyEditor()).toMatchObject({ content: "kept\n" });
  });

  it("shows why Keep mine failed over the banner", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    await changeOnDisk(file, "theirs\n");
    const rebase = vi
      .spyOn(editor, "rebaseEditorBuffer")
      .mockRejectedValue(
        new Error("invalid request: files cannot be created inside .git"),
      );
    try {
      expect(await store().keepActiveEditorBuffer()).toBe(false);
    } finally {
      rebase.mockRestore();
    }

    const failed =
      "Couldn't keep your version: Sage doesn't create Files there (inside protected folders such as .git, or through a link).";
    expect(store().error).toBe(failed);
    expect(readyEditor()).toMatchObject({ saving: false, content: "mine\n" });
    render(
      createElement(FileConflictNotice, {
        document: readyEditor().document,
        editable: true,
      }),
    );
    const alerts = screen
      .getAllByRole("alert")
      .map((alert) => alert.textContent);
    expect(alerts).toContain(failed);
    expect(alerts.some((text) => text?.includes("External change"))).toBe(true);
    cleanup();
  });

  it("Keep mine on a deleted File that came back shows it as changed", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    await changeOnDisk(file, null);
    // It comes back, with no notice.
    browserEditor.changeOnDisk(file, "theirs\n");

    expect(await store().keepActiveEditorBuffer()).toBe(false);
    expect(readyEditor().document.conflict?.kind).toBe("external-change");
    expect(readyEditor()).toMatchObject({ content: "mine\n" });

    expect(await store().keepActiveEditorBuffer()).toBe(true);
    expect(await store().saveActiveEditor()).toBe(true);
    expect(await diskText(file)).toBe("mine\n");
  });

  it("a deleted File that came back is read again: clean reloads, dirty conflicts", async () => {
    const clean = nextFile();
    openTabs(clean);
    await store().loadEditor(clean);
    await changeOnDisk(clean, null);
    await changeOnDisk(clean, "back\n");
    expect(readyEditor()).toMatchObject({ content: "back\n" });
    expect(readyEditor().document.conflict).toBeNull();

    const dirty = nextFile();
    openTabs(dirty);
    await edit(dirty, "mine\n");
    await changeOnDisk(dirty, null);
    await changeOnDisk(dirty, "back\n");
    expect(readyEditor()).toMatchObject({ content: "mine\n" });
    expect(readyEditor().document.conflict?.kind).toBe("external-change");
  });

  it.each([
    ["a recovery copy", true],
    ["edits only in the editor", false],
  ])("Reload of a File gone before its notice keeps %s", async (_, durable) => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    if (durable) await store().flushActiveEditor();
    // Gone, and the notice has not arrived yet.
    browserEditor.changeOnDisk(file, null);

    expect(await store().discardActiveEditor(true)).toBe(false);

    expect(readyEditor()).toMatchObject({ content: "mine\n" });
    expect(readyEditor().document.conflict?.kind).toBe("deleted");
    expect(tabOf(file).dirty).toBe(true);
    if (durable) {
      expect((await editor.openEditorDocument(file)).buffer.content).toBe(
        "mine\n",
      );
    }
  });

  it("Close on a deleted File leaves no recovery buffer", async () => {
    const file = nextFile();
    openTabs(file);
    await edit(file, "mine\n");
    await store().flushActiveEditor();
    await changeOnDisk(file, null);

    await store().closeEditorWithoutSaving(tabOf(file).id);

    expect(store().tabs.some((tab) => sameResource(tab.resource, file))).toBe(
      false,
    );
    // With no buffer and no File, there is nothing left to open.
    await expect(async () => editor.openEditorDocument(file)).rejects.toThrow(
      "does not exist",
    );
  });

  it("shows a deleted File's buffer when its background tab is activated", async () => {
    const [front, back] = [nextFile(), nextFile()];
    openTabs(front, back);
    await store().loadEditor(front);
    await backgroundBuffer(back, "mine\n");
    browserEditor.changeOnDisk(back, null);

    await store().activateTab(tabOf(back).id);

    expect(readyEditor()).toMatchObject({ resource: back, content: "mine\n" });
    expect(readyEditor().document.conflict?.kind).toBe("deleted");
    expect(tabOf(back).status).toBe("failed");
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

  it("names why each File failed and forgets the ones saved since", async () => {
    const [deleted, broken] = [nextFile(), nextFile()];
    openTabs(deleted, broken);
    await edit(deleted, "kept\n");
    await changeOnDisk(deleted, null);
    await backgroundBuffer(broken, "broken\n");
    const save = editor.saveEditorDocument;
    const failing = vi
      .spyOn(editor, "saveEditorDocument")
      .mockImplementation((resource) =>
        sameResource(resource, broken)
          ? Promise.reject(new Error("The disk is full."))
          : save(resource),
      );

    const failures = await store().saveAllEditors();
    failing.mockRestore();

    expect(failures).toEqual([
      { resource: deleted, message: DELETED_ON_DISK },
      { resource: broken, message: "The disk is full." },
    ]);
    expect(tabOf(broken)).toMatchObject({ dirty: true, status: "failed" });

    expect(await store().keepActiveEditorBuffer()).toBe(true);
    expect(await store().saveActiveEditor()).toBe(true);
    expect(store().saveAllReport?.failures).toEqual([
      { resource: broken, message: "The disk is full." },
    ]);
  });
});
