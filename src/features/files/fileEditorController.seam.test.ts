import { readFileSync } from "node:fs";
import path from "node:path";

import { Text } from "@codemirror/state";
import { describe, expect, it, vi } from "vitest";

import type { EditorDocument } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import {
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

describe("file editor controller seam", () => {
  it("uses one shared controller to coordinate load, flush, save, and discard", async () => {
    vi.useFakeTimers();
    const editor = fakeEditorPort();
    vi.mocked(editor.openEditorDocument).mockResolvedValue(document);
    vi.mocked(editor.snapshotEditorBuffer).mockResolvedValue({
      revision: 1,
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

  it("keeps FileSurface independent of desktop and EditorPort runtime imports", () => {
    const source = readFileSync(
      path.resolve(import.meta.dirname, "./FileSurface.tsx"),
      "utf8",
    );

    expect(source).not.toMatch(/from ["'][^"']*services\/desktop["']/);
    expect(source).not.toMatch(/from ["'][^"']*ports\/workbenchRuntime["']/);
    expect(source).not.toMatch(/from ["'][^"']*ports\/workbenchPorts["']/);
    expect(source).not.toContain("bindFileEditorController");
    expect(source).toContain("useFileSurfaceModel");
    const modelSource = readFileSync(
      path.resolve(import.meta.dirname, "./useFileSurfaceModel.ts"),
      "utf8",
    );
    expect(modelSource).toContain("getFileEditorController");
  });

  it("keeps snapshot requests, bounded flushes, revisions, and dirty state in the controller", () => {
    const controllerSource = readFileSync(
      path.resolve(import.meta.dirname, "./fileEditorController.ts"),
      "utf8",
    );
    const sliceSource = readFileSync(
      path.resolve(import.meta.dirname, "../../app/editorSlice.ts"),
      "utf8",
    );

    expect(controllerSource).toContain("BufferSnapshotRequest");
    expect(controllerSource).toContain("MAX_SNAPSHOTS_PER_FLUSH");
    expect(controllerSource).toContain("persistedRevision");
    expect(sliceSource).toContain("getFileEditorController");
    expect(sliceSource).not.toContain("bindFileEditorController");
    expect(sliceSource).not.toContain("BufferSnapshotRequest");
    expect(sliceSource).not.toContain("MAX_SNAPSHOTS_PER_FLUSH");
    expect(sliceSource).not.toContain("snapshotEditorBuffer");
    expect(sliceSource).not.toContain("persistedRevision");
    expect(sliceSource).not.toContain("revision:");
  });
});
