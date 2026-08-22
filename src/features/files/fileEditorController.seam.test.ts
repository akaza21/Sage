import { readFileSync } from "node:fs";

import { describe, expect, it, vi } from "vitest";

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
  it("forwards save and discard to the injected EditorPort", async () => {
    const editor = fakeEditorPort();
    const controller = bindFileEditorController(editor);

    await controller.save(savedResource);
    await controller.discard(discardedResource);

    expect(editor.saveEditorDocument).toHaveBeenCalledWith({
      rootId: "root-notes",
      relativePath: "journal/today.md",
    });
    expect(editor.discardEditorBuffer).toHaveBeenCalledWith({
      rootId: "root-scratch",
      relativePath: "drafts/idea.txt",
    });
  });

  it("keeps FileSurface independent of desktop and EditorPort runtime imports", () => {
    const source = readFileSync(
      new URL("./FileSurface.tsx", import.meta.url),
      "utf8",
    );

    expect(source).not.toMatch(/from ["'][^"']*services\/desktop["']/);
    expect(source).not.toMatch(/from ["'][^"']*ports\/workbenchRuntime["']/);
    expect(source).not.toMatch(/from ["'][^"']*ports\/workbenchPorts["']/);
  });
});
