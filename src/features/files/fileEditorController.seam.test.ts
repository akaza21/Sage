import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it, vi } from "vitest";

import type {
  BufferSnapshotRequest,
  EditorDocument,
} from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import { bindFileEditorController } from "./fileEditorController";

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
const snapshot: BufferSnapshotRequest = {
  resource: savedResource,
  base: version,
  lineEnding: "lf",
  content: "edited\n",
  revision: 1,
};

function fakeEditorPort(): EditorPort {
  return {
    readTextFile: vi.fn(),
    openEditorDocument: vi.fn(),
    snapshotEditorBuffer: vi.fn(),
    saveEditorDocument: vi.fn(),
    discardEditorBuffer: vi.fn(),
  } as EditorPort;
}

describe("file editor controller seam", () => {
  it("coordinates injected EditorPort load, flush, save, and discard", async () => {
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

    await expect(controller.load(savedResource)).resolves.toBe(document);
    controller.scheduleSnapshot(() => controller.snapshot(snapshot));
    await vi.advanceTimersByTimeAsync(750);
    await controller.save(savedResource, () => controller.snapshot(snapshot));
    controller.scheduleSnapshot(() => controller.snapshot(snapshot));
    await controller.discard(discardedResource);

    expect(editor.openEditorDocument).toHaveBeenCalledWith(savedResource);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledTimes(2);
    expect(editor.snapshotEditorBuffer).toHaveBeenLastCalledWith(snapshot);
    expect(editor.saveEditorDocument).toHaveBeenCalledWith({
      rootId: "root-notes",
      relativePath: "journal/today.md",
    });
    expect(editor.discardEditorBuffer).toHaveBeenCalledWith({
      rootId: "root-scratch",
      relativePath: "drafts/idea.txt",
    });
    await vi.advanceTimersByTimeAsync(750);
    expect(editor.snapshotEditorBuffer).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  it("keeps FileSurface independent of desktop and EditorPort runtime imports", () => {
    const source = readFileSync(
      path.resolve(import.meta.dirname, "./FileSurface.tsx"),
      "utf8",
    );

    expect(source).not.toMatch(/from ["'][^"']*services\/desktop["']/);
    expect(source).not.toMatch(/from ["'][^"']*ports\/workbenchRuntime["']/);
    expect(source).not.toMatch(/from ["'][^"']*ports\/workbenchPorts["']/);
  });
});
