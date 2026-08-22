import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it, vi } from "vitest";

import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  createInjectedEditorWatchDelivery,
  type EditorExternalWatchNotice,
} from "../../domain/editorWatch";
import type { EditorDocument } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import {
  applyEditorExternalWatchNotice,
  bindEditorExternalWatch,
} from "./editorExternalWatch";
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

describe("IDE-2 editor external watch seam", () => {
  it("observes a native watch notice without peekEditorDiskVersion", async () => {
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

    const watchSource = readFileSync(
      path.resolve(import.meta.dirname, "./editorExternalWatch.ts"),
      "utf8",
    );
    const controllerSource = readFileSync(
      path.resolve(import.meta.dirname, "./fileEditorController.ts"),
      "utf8",
    );
    expect(watchSource).not.toContain("peekEditorDiskVersion");
    expect(controllerSource).not.toContain("peekEditorDiskVersion");
    expect(watchSource).toContain("EDITOR_EXTERNAL_WATCH_EVENT");
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
});
