import { readFileSync } from "node:fs";
import path from "node:path";

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";

import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  createInjectedEditorWatchDelivery,
  type EditorExternalWatchNotice,
} from "../../domain/editorWatch";
import type { EditorDocument, WorkbenchTab } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import {
  EDITOR_WATCH_LIMIT_ERROR,
  WATCH_LIMIT_MESSAGE,
  applyEditorExternalWatchNotice,
  bindEditorExternalWatch,
  isEditorWatchLimitError,
  useEditorExternalWatch,
} from "./editorExternalWatch";
import { FileSurface } from "./FileSurface";
import {
  bindFileEditorController,
  type EditorStateBridge,
} from "./fileEditorController";

const resource = { rootId: "root-notes", relativePath: "journal/today.md" };
const version = { fingerprint: "base", byteLen: 5, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: {
    fingerprint: "base",
    byteLen: 5,
    revision: 0,
    content: "base\n",
  },
  lineEnding: "lf",
  dirty: false,
  conflict: null,
};

function fakeEditorPort(): EditorPort {
  return {
    readTextFile: vi.fn(),
    openEditorDocument: vi.fn().mockResolvedValue(document),
    snapshotEditorBuffer: vi.fn(),
    saveEditorDocument: vi.fn(),
    discardEditorBuffer: vi.fn(),
    watchEditorDocument: vi.fn(),
  };
}

const inactiveBridge: EditorStateBridge = {
  getActiveEditor: () => null,
  setActiveEditor: () => undefined,
};

function ExternalWatchHarness({
  watchedResource,
}: {
  watchedResource: typeof resource;
}) {
  useEditorExternalWatch(watchedResource, inactiveBridge, () => null);
  return null;
}

const initialWorkbench = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialWorkbench, true);
  vi.restoreAllMocks();
});

describe("IDE-2 editor external watch seam", () => {
  it("watches its active File resource and cancels when the canvas unmounts", async () => {
    const editor = getWorkbenchPorts().editor;
    const injected = createInjectedEditorWatchDelivery();
    const cancel = vi.fn().mockResolvedValue(undefined);
    vi.spyOn(editor, "watchEditorDocument").mockResolvedValue({
      resource,
      cancel,
      delivery: injected.delivery,
    });

    const { unmount } = render(
      createElement(ExternalWatchHarness, { watchedResource: resource }),
    );

    await waitFor(() => {
      expect(editor.watchEditorDocument).toHaveBeenCalledWith(resource);
    });
    unmount();

    await waitFor(() => {
      expect(cancel).toHaveBeenCalledOnce();
    });
  });

  it("waits for the prior native cancellation before replacing a File watch", async () => {
    const editor = getWorkbenchPorts().editor;
    let finishFirstCancel: () => void = () => undefined;
    const firstCancellation = new Promise<void>((resolve) => {
      finishFirstCancel = resolve;
    });
    const firstCancel = vi.fn(() => firstCancellation);
    const secondCancel = vi.fn().mockResolvedValue(undefined);
    const firstDelivery = createInjectedEditorWatchDelivery();
    const secondDelivery = createInjectedEditorWatchDelivery();
    const secondResource = { rootId: "root-notes", relativePath: "next.txt" };
    const watch = vi
      .spyOn(editor, "watchEditorDocument")
      .mockResolvedValueOnce({
        resource,
        cancel: firstCancel,
        delivery: firstDelivery.delivery,
      })
      .mockResolvedValueOnce({
        resource: secondResource,
        cancel: secondCancel,
        delivery: secondDelivery.delivery,
      });

    const { rerender, unmount } = render(
      createElement(ExternalWatchHarness, { watchedResource: resource }),
    );
    await waitFor(() => {
      expect(watch).toHaveBeenCalledTimes(1);
    });

    act(() => {
      rerender(
        createElement(ExternalWatchHarness, {
          watchedResource: secondResource,
        }),
      );
    });
    await waitFor(() => {
      expect(firstCancel).toHaveBeenCalledOnce();
    });
    expect(watch).toHaveBeenCalledTimes(1);

    finishFirstCancel();
    await waitFor(() => {
      expect(watch).toHaveBeenCalledWith(secondResource);
    });
    unmount();
    await waitFor(() => {
      expect(secondCancel).toHaveBeenCalledOnce();
    });
  });

  it("keeps a capped File editable and retries only when its editor takes focus", async () => {
    const editor = getWorkbenchPorts().editor;
    const delivery = createInjectedEditorWatchDelivery();
    let slotFree = false;
    const watch = vi
      .spyOn(editor, "watchEditorDocument")
      .mockImplementation(async () => {
        if (!slotFree) {
          throw EDITOR_WATCH_LIMIT_ERROR;
        }
        slotFree = false;
        return {
          resource,
          delivery: delivery.delivery,
          cancel: async () => {
            slotFree = true;
          },
        };
      });
    const fileTab: WorkbenchTab = {
      id: `file:${resource.relativePath}`,
      kind: "file",
      title: resource.relativePath,
      position: 1,
      pinned: false,
      dirty: false,
      resource,
    };
    useWorkbenchStore.setState({
      tabs: [fileTab],
      activeTabId: fileTab.id,
      activeEditor: bindFileEditorController(editor).ready(document),
    });

    const { unmount } = render(
      createElement(
        "div",
        null,
        createElement("button", { type: "button" }, "Elsewhere"),
        createElement(FileSurface, { tab: fileTab }),
      ),
    );
    await screen.findByText(WATCH_LIMIT_MESSAGE);
    expect(watch).toHaveBeenCalledTimes(1);
    expect(useWorkbenchStore.getState().tabs[0]?.status).toBeUndefined();
    const fileEditor = await screen.findByRole("textbox", {
      name: `Edit ${resource.relativePath}`,
    });
    expect(fileEditor).not.toHaveAttribute("contenteditable", "false");

    // Still at the cap: focusing the editor retries and fails, and editing
    // and snapshotting the File do not clear the warning while it is unwatched.
    const snapshot = vi
      .spyOn(editor, "snapshotEditorBuffer")
      .mockImplementation(async (request) => ({
        revision: request.revision,
        persistedAt: 1,
        dirty: true,
      }));
    fireEvent.focusIn(fileEditor);
    await waitFor(() => {
      expect(watch).toHaveBeenCalledTimes(2);
    });
    await act(async () => undefined);
    act(() => {
      useWorkbenchStore.getState().updateEditorContent("base\nedited\n");
    });
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      message: null,
    });
    expect(screen.getByText(WATCH_LIMIT_MESSAGE)).toBeInTheDocument();
    await act(async () => {
      await useWorkbenchStore.getState().flushActiveEditor();
    });
    expect(snapshot).toHaveBeenCalled();
    expect(screen.getByText(WATCH_LIMIT_MESSAGE)).toBeInTheDocument();

    // A slot is free, but focus elsewhere in the app does not ask for it.
    slotFree = true;
    fireEvent.focusIn(screen.getByRole("button", { name: "Elsewhere" }));
    await act(async () => undefined);
    expect(watch).toHaveBeenCalledTimes(2);
    expect(screen.getByText(WATCH_LIMIT_MESSAGE)).toBeInTheDocument();

    // Focus moving within the editor has not entered it, so it does not ask.
    fireEvent.focusIn(fileEditor, { relatedTarget: fileEditor });
    await act(async () => undefined);
    expect(watch).toHaveBeenCalledTimes(2);

    fireEvent.focusIn(fileEditor, {
      relatedTarget: screen.getByRole("button", { name: "Elsewhere" }),
    });
    await waitFor(() => {
      expect(screen.queryByText(WATCH_LIMIT_MESSAGE)).toBeNull();
    });
    expect(watch).toHaveBeenCalledTimes(3);
    expect(useWorkbenchStore.getState().tabs[0]?.status).toBeUndefined();

    // Once watched, further focus changes in the editor send no more requests.
    fireEvent.focusIn(fileEditor);
    await act(async () => undefined);
    expect(watch).toHaveBeenCalledTimes(3);

    act(() => delivery.emit({ resource, generation: 1, kind: "deleted" }));
    await screen.findByText(/This file was deleted outside Sage/);
    unmount();
    await waitFor(() => expect(slotFree).toBe(true));
  });

  it("recognises the native watch-limit code by name, not by display text", () => {
    const nativeSource = readFileSync(
      path.resolve(
        import.meta.dirname,
        "../../../src-tauri/src/editor_watch.rs",
      ),
      "utf8",
    );
    expect(nativeSource).toContain(
      `pub const EDITOR_WATCH_LIMIT_ERROR: &str = "${EDITOR_WATCH_LIMIT_ERROR}";`,
    );
    expect(isEditorWatchLimitError(EDITOR_WATCH_LIMIT_ERROR)).toBe(true);
    expect(
      isEditorWatchLimitError(
        "resource limit exceeded: too many active editor file watches",
      ),
    ).toBe(false);
  });

  it("delivers a native watch notice for its File to the subscriber", async () => {
    expect(EDITOR_EXTERNAL_WATCH_EVENT).toBe("editor-external-watch-v1");
    const editor = fakeEditorPort();
    const injected = createInjectedEditorWatchDelivery();
    vi.mocked(editor.watchEditorDocument).mockResolvedValue({
      resource,
      cancel: vi.fn().mockResolvedValue(undefined),
      delivery: injected.delivery,
    });

    const seen: EditorExternalWatchNotice[] = [];
    const subscription = await bindEditorExternalWatch(editor, resource, {
      onNotice: (notice) => {
        seen.push(notice);
      },
    });
    const notice: EditorExternalWatchNotice = {
      resource,
      generation: 1,
      kind: "changed",
    };
    injected.emit(notice);

    expect(seen).toEqual([notice]);
    expect(subscription.delivery).toBe(injected.delivery);
    expect(subscription.resource).toEqual(resource);
  });

  it("reloads a clean File with the new disk bytes when the watch reports a change", async () => {
    const editor = getWorkbenchPorts().editor;
    const injected = createInjectedEditorWatchDelivery();
    vi.spyOn(editor, "watchEditorDocument").mockResolvedValue({
      resource,
      cancel: vi.fn().mockResolvedValue(undefined),
      delivery: injected.delivery,
    });
    const changedVersion = {
      fingerprint: "changed-outside",
      byteLen: 15,
      modifiedAt: 9,
    };
    const changedOnDisk: EditorDocument = {
      ...document,
      disk: changedVersion,
      base: changedVersion,
      buffer: {
        fingerprint: changedVersion.fingerprint,
        byteLen: changedVersion.byteLen,
        revision: 0,
        content: "edited outside\n",
      },
    };
    let disk = document;
    vi.spyOn(editor, "openEditorDocument").mockImplementation(async () => disk);
    const controller = bindFileEditorController(editor);
    let activeEditor = controller.ready(document);
    const updates: { dirty?: boolean; tabStatus?: "failed" | null }[] = [];
    const bridge: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next, update) => {
        activeEditor = next;
        if (update) updates.push(update);
      },
    };
    function WatchedEditor() {
      useEditorExternalWatch(resource, bridge, () => null);
      return null;
    }
    render(createElement(WatchedEditor));
    await waitFor(() => {
      expect(editor.watchEditorDocument).toHaveBeenCalledWith(resource);
    });

    // The bytes change outside Sage and the native watch reports it.
    disk = changedOnDisk;
    act(() => {
      injected.emit({ resource, generation: 1, kind: "changed" });
    });
    await waitFor(() => {
      expect(activeEditor.phase === "ready" && activeEditor.content).toBe(
        "edited outside\n",
      );
    });
    expect(activeEditor.phase === "ready" && activeEditor.document).toEqual(
      changedOnDisk,
    );
    expect(updates.at(-1)).toEqual({ dirty: false, tabStatus: null });
  });

  it("does not reopen a dirty buffer when a deletion notice arrives", async () => {
    const editor = fakeEditorPort();
    const controller = bindFileEditorController(editor);
    let activeEditor = controller.ready(document);
    const bridge: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next) => {
        activeEditor = next;
      },
    };
    controller.updateContent(bridge, "unsaved dirty buffer\n");
    expect(activeEditor.phase).toBe("ready");
    const dirtyContent =
      activeEditor.phase === "ready" ? activeEditor.content : "";

    await applyEditorExternalWatchNotice(
      { resource, generation: 2, kind: "deleted" },
      bridge,
      editor,
    );

    expect(editor.openEditorDocument).not.toHaveBeenCalled();
    expect(activeEditor.phase).toBe("ready");
    if (activeEditor.phase === "ready") {
      expect(activeEditor.content).toBe(dirtyContent);
      expect(activeEditor.document.conflict?.kind).toBe("external-change");
    }
  });

  it("does not overwrite edits that land while a changed reopen is in flight", async () => {
    let resolveOpen: (document: EditorDocument) => void = () => undefined;
    const pending = new Promise<EditorDocument>((resolve) => {
      resolveOpen = resolve;
    });
    const editor = fakeEditorPort();
    vi.mocked(editor.openEditorDocument).mockReturnValue(pending);
    const controller = bindFileEditorController(editor);
    let activeEditor = controller.ready(document);
    const bridge: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next) => {
        activeEditor = next;
      },
    };

    const apply = applyEditorExternalWatchNotice(
      { resource, generation: 3, kind: "changed" },
      bridge,
      editor,
    );
    controller.updateContent(bridge, "typed during reopen\n");
    const dirtyRevision =
      activeEditor.phase === "ready" ? activeEditor.revision : -1;
    resolveOpen({
      ...document,
      buffer: {
        ...document.buffer,
        fingerprint: "from-disk",
        revision: 99,
        content: "from disk\n",
      },
    });
    await apply;

    expect(activeEditor.phase).toBe("ready");
    if (activeEditor.phase === "ready") {
      expect(activeEditor.content).toBe("typed during reopen\n");
      expect(activeEditor.revision).toBe(dirtyRevision);
      expect(activeEditor.document.conflict?.kind).toBe("external-change");
    }
  });

  it("reconciles Sage's completed save but still rejects different disk bytes", async () => {
    let resolveOpen: (document: EditorDocument) => void = () => undefined;
    const pending = new Promise<EditorDocument>((resolve) => {
      resolveOpen = resolve;
    });
    const editor = fakeEditorPort();
    vi.mocked(editor.openEditorDocument).mockReturnValue(pending);
    const savedVersion = {
      fingerprint: "saved",
      byteLen: 6,
      modifiedAt: 2,
    };
    const savedDocument: EditorDocument = {
      ...document,
      disk: savedVersion,
      base: savedVersion,
      buffer: {
        fingerprint: savedVersion.fingerprint,
        byteLen: savedVersion.byteLen,
        revision: 0,
        content: "saved\n",
      },
      dirty: false,
      conflict: null,
    };
    let activeEditor = {
      phase: "ready" as const,
      resource,
      document,
      content: "saved\n",
      revision: 1,
      persistedRevision: 1,
      persisting: false,
      saving: true,
      message: null,
    };
    const tabStatuses: Array<string | null | undefined> = [];
    const bridge: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next, options) => {
        activeEditor = next as typeof activeEditor;
        tabStatuses.push(options?.tabStatus);
      },
    };

    const apply = applyEditorExternalWatchNotice(
      { resource, generation: 4, kind: "changed" },
      bridge,
      editor,
    );
    bridge.setActiveEditor({
      phase: "ready",
      resource,
      document: savedDocument,
      content: savedDocument.buffer.content,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    });
    resolveOpen(savedDocument);
    await apply;

    expect(activeEditor.document.conflict).toBeNull();
    expect(tabStatuses).not.toContain("failed");

    const externalVersion = {
      fingerprint: "external",
      byteLen: 9,
      modifiedAt: 3,
    };
    const externalDocument: EditorDocument = {
      ...savedDocument,
      disk: externalVersion,
      base: externalVersion,
      buffer: {
        fingerprint: externalVersion.fingerprint,
        byteLen: externalVersion.byteLen,
        revision: 0,
        content: "external\n",
      },
    };
    activeEditor = {
      phase: "ready",
      resource,
      document,
      content: "saved\n",
      revision: 2,
      persistedRevision: 2,
      persisting: false,
      saving: true,
      message: null,
    };
    vi.mocked(editor.openEditorDocument).mockResolvedValue(externalDocument);
    const applyExternal = applyEditorExternalWatchNotice(
      { resource, generation: 5, kind: "changed" },
      bridge,
      editor,
    );
    bridge.setActiveEditor({
      phase: "ready",
      resource,
      document: savedDocument,
      content: savedDocument.buffer.content,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    });
    await applyExternal;

    expect(activeEditor.document.conflict?.kind).toBe("external-change");
    expect(tabStatuses).toContain("failed");
  });

  it("stops delivering notices after cancel", async () => {
    const editor = fakeEditorPort();
    const injected = createInjectedEditorWatchDelivery();
    vi.mocked(editor.watchEditorDocument).mockResolvedValue({
      resource,
      cancel: vi.fn().mockResolvedValue(undefined),
      delivery: injected.delivery,
    });

    const seen: EditorExternalWatchNotice[] = [];
    const subscription = await bindEditorExternalWatch(editor, resource, {
      onNotice: (notice) => {
        seen.push(notice);
      },
    });
    injected.emit({ resource, generation: 1, kind: "changed" });
    expect(seen).toHaveLength(1);

    await subscription.cancel();
    injected.emit({ resource, generation: 2, kind: "changed" });
    injected.emit({ resource, generation: 3, kind: "deleted" });

    expect(seen).toHaveLength(1);
    expect(editor.openEditorDocument).not.toHaveBeenCalled();
  });
});
