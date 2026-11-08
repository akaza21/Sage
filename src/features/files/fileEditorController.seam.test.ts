import { Text } from "@codemirror/state";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { EditorDocument } from "../../domain/workbench";
import { createBrowserWorkbenchPorts } from "../../ports/browserWorkbenchPorts";
import type { EditorPort } from "../../ports/workbenchPorts";
import {
  type ActiveEditorState,
  bindFileEditorController,
  type EditorStateBridge,
  getFileEditorController,
} from "./fileEditorController";

const savedResource = {
  rootId: "root-notes",
  relativePath: "journal/today.md",
};
const discardedResource = {
  rootId: "root-scratch",
  relativePath: "drafts/idea.txt",
};
const version = { fingerprint: "base", byteLen: 5, modifiedAt: null };
const document: EditorDocument = {
  resource: savedResource,
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
    openEditorDocument: vi.fn(),
    snapshotEditorBuffer: vi.fn(),
    saveEditorDocument: vi.fn(),
    discardEditorBuffer: vi.fn(),
    watchEditorDocument: vi.fn(),
  } as EditorPort;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("file editor controller seam", () => {
  it("uses one shared controller to coordinate load, flush, save, and discard", async () => {
    vi.useFakeTimers();
    const editor = fakeEditorPort();
    vi.mocked(editor.openEditorDocument).mockResolvedValue(document);
    vi.mocked(editor.snapshotEditorBuffer).mockResolvedValue({
      revision: 1,
      applied: true,
      persistedAt: 1,
      dirty: true,
    });
    vi.mocked(editor.saveEditorDocument).mockResolvedValue({
      outcome: "saved",
      document,
    });
    vi.mocked(editor.discardEditorBuffer).mockResolvedValue(document);
    const controller = bindFileEditorController(editor);
    let activeEditor = controller.ready(document);
    const editorState: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next) => {
        activeEditor = next;
      },
    };

    expect(getFileEditorController()).toBe(controller);

    await expect(controller.load(savedResource)).resolves.toBe(document);
    controller.updateContent(editorState, "edited\n");
    controller.scheduleSnapshot(() => controller.flush(editorState));
    await controller.save(editorState, () => true);
    await vi.advanceTimersByTimeAsync(750);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledTimes(1);

    controller.scheduleSnapshot(() => controller.flush(editorState));
    activeEditor = controller.ready({
      ...document,
      resource: discardedResource,
    });
    await controller.discard(editorState, () => true);

    expect(editor.openEditorDocument).toHaveBeenCalledWith(savedResource);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledTimes(1);
    expect(editor.snapshotEditorBuffer).toHaveBeenLastCalledWith(
      expect.objectContaining({
        resource: savedResource,
        content: "edited\n",
        revision: 1,
      }),
    );
    expect(editor.saveEditorDocument).toHaveBeenCalledWith({
      rootId: "root-notes",
      relativePath: "journal/today.md",
    });
    expect(editor.discardEditorBuffer).toHaveBeenCalledWith({
      rootId: "root-scratch",
      relativePath: "drafts/idea.txt",
    });
    await vi.advanceTimersByTimeAsync(750);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("takes the editor's answer for dirty state and builds text only for snapshots", async () => {
    const editor = fakeEditorPort();
    vi.mocked(editor.snapshotEditorBuffer).mockImplementation(
      async (request) => ({
        revision: request.revision,
        applied: true,
        persistedAt: 1,
        dirty: true,
      }),
    );
    const controller = bindFileEditorController(editor);
    let activeEditor = controller.ready(document);
    const dirtyUpdates: (boolean | undefined)[] = [];
    const editorState: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next, update) => {
        activeEditor = next;
        if (update?.dirty !== undefined) dirtyUpdates.push(update.dirty);
      },
    };
    // Same length as the buffer: only the editor's answer avoids a string.
    const sameLength = Text.of(["bass", ""]);
    const toString = vi.spyOn(Text.prototype, "toString");
    controller.updateContent(editorState, sameLength, {
      content: "base\n",
      unchanged: false,
    });
    const typedBack = Text.of(["base", ""]);
    controller.updateContent(editorState, typedBack, {
      content: "base\n",
      unchanged: true,
    });
    expect(dirtyUpdates).toEqual([true, false]);
    expect(toString).not.toHaveBeenCalled();

    // An answer about some other text is ignored; the text is compared.
    controller.updateContent(editorState, sameLength, {
      content: "other",
      unchanged: true,
    });
    expect(dirtyUpdates.at(-1)).toBe(true);

    // Without an answer, a length change is enough to call the File dirty.
    toString.mockClear();
    controller.updateContent(editorState, Text.of(["base", "x"]));
    expect(dirtyUpdates.at(-1)).toBe(true);
    expect(toString).not.toHaveBeenCalled();
    controller.updateContent(editorState, sameLength);

    await controller.flush(editorState);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledWith(
      expect.objectContaining({ content: "bass\n", revision: 5 }),
    );
    toString.mockRestore();
  });

  it("persists the latest edit, bounds a flush that never catches up, and saves the exact bytes", async () => {
    const editor = createBrowserWorkbenchPorts().editor;
    const resource = {
      rootId: "root-controller-seam",
      relativePath: "notes/bounded.txt",
    };
    const controller = bindFileEditorController(editor);
    let activeEditor: ActiveEditorState | null = null;
    const dirtyUpdates: boolean[] = [];
    const bridge: EditorStateBridge = {
      getActiveEditor: () => activeEditor,
      setActiveEditor: (next, update) => {
        activeEditor = next;
        if (update?.dirty !== undefined) dirtyUpdates.push(update.dirty);
      },
    };
    const ready = () => {
      if (activeEditor?.phase !== "ready") throw new Error("not ready");
      return activeEditor;
    };
    try {
      const opened = await controller.load(resource, bridge);
      const original = opened.buffer.content;

      // Two edits, one flush: only the latest text is persisted.
      controller.updateContent(bridge, "first draft\n");
      controller.updateContent(bridge, "second draft\n");
      await controller.flush(bridge);
      expect(ready()).toMatchObject({
        revision: 2,
        persistedRevision: 2,
        persisting: false,
      });
      expect(dirtyUpdates.at(-1)).toBe(true);
      const reopened = await editor.openEditorDocument(resource);
      expect(reopened.buffer).toMatchObject({
        content: "second draft\n",
        revision: 2,
      });
      expect(reopened.dirty).toBe(true);
      expect((await editor.readTextFile(resource)).content).toBe(original);

      // A writer that edits again whenever a snapshot lands never lets the
      // flush catch up: it stops with an error after a bounded number of
      // snapshots instead of looping, and keeps the newest acknowledged text.
      const persist = editor.snapshotEditorBuffer.bind(editor);
      const typing = vi
        .spyOn(editor, "snapshotEditorBuffer")
        .mockImplementation(async (request) => {
          const ack = await persist(request);
          controller.updateContent(bridge, `typed after ${request.revision}\n`);
          return ack;
        });
      controller.updateContent(bridge, "third draft\n");
      await expect(controller.flush(bridge)).rejects.toThrow(/revision limit/);
      typing.mockRestore();
      const afterBound = ready();
      expect(afterBound.revision).toBe(afterBound.persistedRevision + 1);
      expect(afterBound.persisting).toBe(false);
      expect((await editor.openEditorDocument(resource)).buffer).toMatchObject({
        content: `typed after ${afterBound.persistedRevision - 1}\n`,
        revision: afterBound.persistedRevision,
      });

      // Save flushes what is still pending, then writes exactly those bytes.
      controller.updateContent(bridge, "saved bytes\n");
      const result = await controller.save(bridge, () => true);
      expect(result?.outcome).toBe("saved");
      expect((await editor.readTextFile(resource)).content).toBe(
        "saved bytes\n",
      );
      expect(ready()).toMatchObject({
        content: "saved bytes\n",
        saving: false,
        persisting: false,
      });
      expect(ready().revision).toBe(ready().persistedRevision);
      expect(ready().document.dirty).toBe(false);
      expect(dirtyUpdates.at(-1)).toBe(false);
      expect((await editor.openEditorDocument(resource)).dirty).toBe(false);
    } finally {
      await editor.discardEditorBuffer(resource);
    }
  });
});
