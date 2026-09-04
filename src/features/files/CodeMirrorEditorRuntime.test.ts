import { undo } from "@codemirror/commands";
import {
  defaultHighlightStyle,
  syntaxTree,
  type TagStyle,
} from "@codemirror/language";
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

describe("CodeMirrorEditorRuntime caret", () => {
  it("reports the caret head after create and setSelection", () => {
    const onCaretOffset = vi.fn();
    const parent = document.createElement("div");
    document.body.append(parent);
    const runtime = createCodeMirrorEditorRuntime({
      parent,
      ariaLabel: "Edit note.txt",
      path: "note.txt",
      content: "alpha\nbeta",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
      onCaretOffset,
    });
    expect(onCaretOffset).toHaveBeenCalledWith(0);
    runtime.setSelection(6, 6);
    expect(onCaretOffset).toHaveBeenLastCalledWith(6);
    runtime.destroy();
  });
});

describe("CodeMirrorEditorRuntime appearance", () => {
  it("reconfigures all five settings without remounting or losing selection and undo history", () => {
    const { parent, runtime, view } = runtimeForPath("notes.txt");
    expect(view.state.tabSize).toBe(2);
    expect(parent.querySelector(".cm-lineNumbers")).not.toBeNull();
    expect(parent.querySelector(".cm-highlightSpace")).toBeNull();
    view.dispatch({ changes: { from: view.state.doc.length, insert: "!" } });
    expect(view.state.doc.toString()).toBe("fn main() {}!");
    runtime.setSelection(3, 7);
    runtime.update({
      ariaLabel: "Edit notes.txt",
      path: "notes.txt",
      content: "fn main() {}!",
      disabled: false,
      maxContentLength: 1024,
      appearance: {
        fontSize: 16,
        tabSize: 4,
        wordWrap: true,
        lineNumbers: false,
        showWhitespace: true,
      },
      onChange: vi.fn(),
    });
    expect(view.state.tabSize).toBe(4);
    expect(runtime.selection()).toEqual({ start: 3, end: 7 });
    expect(EditorView.findFromDOM(view.contentDOM)).toBe(view);
    expect(parent.querySelector(".cm-lineNumbers")).toBeNull();
    expect(parent.querySelector(".cm-highlightSpace")).not.toBeNull();
    expect(view.dom.className).toContain("cm-editor");
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("fn main() {}");

    runtime.update({
      ariaLabel: "Edit notes.txt",
      path: "notes.txt",
      content: "fn main() {}",
      disabled: false,
      maxContentLength: 1024,
      appearance: {
        fontSize: 13,
        tabSize: 2,
        wordWrap: false,
        lineNumbers: true,
        showWhitespace: false,
      },
      onChange: vi.fn(),
    });
    expect(parent.querySelector(".cm-lineNumbers")).not.toBeNull();
    expect(parent.querySelector(".cm-highlightSpace")).toBeNull();
    runtime.destroy();
  });

  it("does not rebuild appearance chrome for an ordinary controlled content update", () => {
    const { parent, runtime } = runtimeForPath("notes.txt");
    const gutter = parent.querySelector(".cm-gutters");

    runtime.update({
      ariaLabel: "Edit notes.txt",
      path: "notes.txt",
      content: "controlled content",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });

    expect(parent.querySelector(".cm-gutters")).toBe(gutter);
    runtime.destroy();
  });
});

describe("documentSyntaxHighlightStyle token wiring", () => {
  const syntaxTokens = [
    "--doc-syntax-meta",
    "--doc-syntax-keyword",
    "--doc-syntax-atom",
    "--doc-syntax-literal",
    "--doc-syntax-string",
    "--doc-syntax-regexp",
    "--doc-syntax-variable-definition",
    "--doc-syntax-variable-local",
    "--doc-syntax-type",
    "--doc-syntax-class",
    "--doc-syntax-variable-special",
    "--doc-syntax-property-definition",
    "--doc-syntax-comment",
    "--doc-syntax-invalid",
  ];

  function tokenDrivenColors(): string[] {
    const found: string[] = [];
    for (const sheet of Array.from(document.styleSheets)) {
      let rules: CSSRuleList | undefined;
      try {
        rules = sheet.cssRules;
      } catch {
        continue;
      }
      for (const rule of Array.from(rules)) {
        if (!(rule instanceof CSSStyleRule)) {
          continue;
        }
        const color = rule.style.getPropertyValue("color");
        if (color.includes("var(--doc-syntax-")) {
          found.push(color);
        }
      }
    }
    return found;
  }

  async function expectDefaultSpecDrift(
    mutate: (specs: readonly TagStyle[]) => readonly TagStyle[],
  ) {
    vi.resetModules();
    vi.doMock("@codemirror/language", async (importOriginal) => {
      const actual = (await importOriginal()) as Record<string, unknown> & {
        defaultHighlightStyle: typeof defaultHighlightStyle;
      };
      return {
        ...actual,
        defaultHighlightStyle: {
          specs: mutate(actual.defaultHighlightStyle.specs),
        },
      };
    });
    try {
      await expect(import("./CodeMirrorEditorRuntime")).rejects.toThrow(
        /refusing to mis-map token colors/,
      );
    } finally {
      vi.doUnmock("@codemirror/language");
      vi.resetModules();
    }
  }

  it("mounts token-driven syntax declarations for every syntax token", () => {
    const { runtime } = runtimeForPath("notes.txt");
    const colors = tokenDrivenColors();
    for (const token of syntaxTokens) {
      expect(colors.join("\n")).toContain(`var(${token})`);
    }
    runtime.destroy();
  });

  it("mounts no raw default palette colors", () => {
    const { runtime } = runtimeForPath("notes.txt");
    const colors = tokenDrivenColors();
    expect(colors.length).toBeGreaterThan(0);
    for (const color of colors) {
      expect(color).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    }
    runtime.destroy();
  });

  it("fails closed when the installed default spec count drifts", () => {
    return expectDefaultSpecDrift(() => [
      { tag: defaultHighlightStyle.specs[0].tag, color: "currentColor" },
    ]);
  });

  it("rejects a same-count reorder of specs with matching style shapes", () => {
    return expectDefaultSpecDrift((specs) => {
      const reordered = [...specs];
      [reordered[11], reordered[12]] = [reordered[12], reordered[11]];
      return reordered;
    });
  });

  it("rejects drift in a non-color style value", () => {
    return expectDefaultSpecDrift((specs) =>
      specs.map((spec, index) =>
        index === 1 ? { ...spec, textDecoration: "overline" } : spec,
      ),
    );
  });

  it("rejects drift in a default palette value", () => {
    return expectDefaultSpecDrift((specs) =>
      specs.map((spec, index) =>
        index === 6 ? { ...spec, color: "currentColor" } : spec,
      ),
    );
  });
});
