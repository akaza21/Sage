import { syntaxTree } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createCodeMirrorEditorRuntime } from "./CodeMirrorEditorRuntime";

function runtimeForPath(path: string) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const runtime = createCodeMirrorEditorRuntime({
    parent,
    ariaLabel: `Edit ${path}`,
    path,
    content: "fn main() {}",
    disabled: false,
    maxContentLength: 1024,
    onChange: vi.fn(),
  });
  const editor = parent.querySelector<HTMLElement>(".cm-content");
  const view = editor ? EditorView.findFromDOM(editor) : null;
  if (!view) {
    throw new Error("Expected a mounted CodeMirror editor");
  }
  return { parent, runtime, view };
}

afterEach(() => {
  document.body.replaceChildren();
  vi.doUnmock("@codemirror/lang-css");
  vi.doUnmock("@codemirror/lang-python");
});

describe("CodeMirrorEditorRuntime language lifecycle", () => {
  it("falls back to plain text when the current language chunk rejects", async () => {
    const chunkRequested = vi.fn();
    vi.doMock("@codemirror/lang-css", () => {
      chunkRequested();
      throw new Error("language chunk unavailable");
    });
    const { runtime, view } = runtimeForPath("src/main.rs");
    await vi.waitFor(() =>
      expect(syntaxTree(view.state).length).toBeGreaterThan(0),
    );

    runtime.update({
      ariaLabel: "Edit styles/app.css",
      path: "styles/app.css",
      content: "fn main() {}",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });

    expect(syntaxTree(view.state).length).toBe(0);
    await vi.waitFor(() => expect(chunkRequested).toHaveBeenCalledOnce());
    await Promise.resolve();
    expect(syntaxTree(view.state).length).toBe(0);
    runtime.destroy();
  });

  it("ignores a superseded language chunk that resolves out of order", async () => {
    let resolvePython:
      ((module: { python: () => Extension }) => void) | undefined;
    vi.doMock(
      "@codemirror/lang-python",
      () =>
        new Promise<{ python: () => Extension }>((resolve) => {
          resolvePython = resolve;
        }),
    );
    const { runtime, view } = runtimeForPath("notes.txt");

    runtime.update({
      ariaLabel: "Edit scripts/slow.py",
      path: "scripts/slow.py",
      content: "fn main() {}",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    await vi.waitFor(() => expect(resolvePython).toBeTypeOf("function"));

    runtime.update({
      ariaLabel: "Edit src/main.rs",
      path: "src/main.rs",
      content: "fn main() {}",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    await vi.waitFor(() =>
      expect(syntaxTree(view.state).length).toBeGreaterThan(0),
    );

    const python = vi.fn<() => Extension>(() => []);
    resolvePython?.({ python });
    await vi.waitFor(() => expect(python).toHaveBeenCalledOnce());
    await Promise.resolve();

    expect(syntaxTree(view.state).length).toBeGreaterThan(0);
    runtime.destroy();
  });
});
