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

function runtimeForPath(
  path: string,
  documentIdentity?: string,
  content = "fn main() {}",
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const runtime = createCodeMirrorEditorRuntime({
    parent,
    ariaLabel: `Edit ${path}`,
    documentIdentity,
    path,
    content,
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

function syntaxNodeNames(view: EditorView): string[] {
  const names: string[] = [];
  syntaxTree(view.state).iterate({
    enter: (node) => {
      names.push(node.name);
    },
  });
  return names;
}

afterEach(() => {
  document.body.replaceChildren();
  vi.doUnmock("./editorLanguageCss");
  vi.doUnmock("./editorLanguagePython");
});

describe("CodeMirrorEditorRuntime language lifecycle", () => {
  it("keeps CSS and Python parsing and language data without autocomplete providers", async () => {
    const css = runtimeForPath(
      "styles/app.css",
      undefined,
      ".card { color: red; }",
    );
    await vi.waitFor(() =>
      expect(syntaxNodeNames(css.view)).toEqual(
        expect.arrayContaining(["Declaration", "PropertyName"]),
      ),
    );
    expect(css.view.state.languageDataAt("commentTokens", 0)).toEqual([
      { block: { close: "*/", open: "/*" } },
    ]);
    expect(css.view.state.languageDataAt("autocomplete", 0)).toEqual([]);
    css.runtime.destroy();

    const python = runtimeForPath(
      "tools/check.py",
      undefined,
      "def check(value):\n  return value",
    );
    await vi.waitFor(() =>
      expect(syntaxNodeNames(python.view)).toEqual(
        expect.arrayContaining(["FunctionDefinition", "VariableName"]),
      ),
    );
    expect(python.view.state.languageDataAt("commentTokens", 0)).toEqual([
      { line: "#" },
    ]);
    expect(python.view.state.languageDataAt("autocomplete", 0)).toEqual([]);
    python.runtime.destroy();
  });

  it("keeps Markdown parsing and language data without autocomplete providers", async () => {
    const { runtime, view } = runtimeForPath(
      "docs/readme.md",
      undefined,
      "# Heading\n\n[documentation](https://example.test)",
    );
    await vi.waitFor(() =>
      expect(syntaxNodeNames(view)).toEqual(
        expect.arrayContaining(["ATXHeading1", "URL"]),
      ),
    );
    expect(view.state.languageDataAt("autocomplete", 0)).toEqual([]);
    runtime.destroy();
  });

  it("keeps no-match HTML parsing and embedded JavaScript and CSS languages", async () => {
    const { runtime, view } = runtimeForPath(
      "public/index.html",
      undefined,
      "<div></span><script>const answer = 1;</script><style>.card { color: red; }</style>",
    );
    await vi.waitFor(() =>
      expect(syntaxNodeNames(view)).toEqual(
        expect.arrayContaining([
          "StartCloseTag",
          "VariableDeclaration",
          "Declaration",
        ]),
      ),
    );
    expect(syntaxNodeNames(view)).not.toContain("MismatchedCloseTag");
    runtime.destroy();
  });

  it("preserves JavaScript JSX and TSX close-tag insertion through CodeMirror input", async () => {
    for (const path of ["view.jsx", "view.tsx"]) {
      const { runtime, view } = runtimeForPath(path, undefined, "<Widget");
      await vi.waitFor(() =>
        expect(syntaxNodeNames(view)).toContain("JSXIdentifier"),
      );
      runtime.setSelection(view.state.doc.length, view.state.doc.length);
      view.contentDOM.dispatchEvent(
        new InputEvent("beforeinput", {
          bubbles: true,
          cancelable: true,
          data: ">",
          inputType: "insertText",
        }),
      );
      view.contentDOM.append(view.dom.ownerDocument.createTextNode(">"));
      (view as unknown as { observer: { flush: () => void } }).observer.flush();
      await vi.waitFor(() =>
        expect(view.state.doc.toString()).toBe("<Widget></Widget>"),
      );
      runtime.destroy();
    }
  });

  it("falls back to plain text when the current language chunk rejects", async () => {
    const chunkRequested = vi.fn();
    vi.doMock("./editorLanguageCss", () => {
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
      | ((module: { loadPythonEditorLanguage: () => Extension }) => void)
      | undefined;
    let resolvePythonLanguageRead: (() => void) | undefined;
    let resolveStaleContinuation: (() => void) | undefined;
    const pythonLanguageRead = new Promise<void>((resolve) => {
      resolvePythonLanguageRead = resolve;
    });
    const staleContinuationSettled = new Promise<void>((resolve) => {
      resolveStaleContinuation = resolve;
    });
    vi.doMock(
      "./editorLanguagePython",
      () =>
        new Promise<{ loadPythonEditorLanguage: () => Extension }>(
          (resolve) => {
            resolvePython = resolve;
          },
        ),
    );
    const { runtime, view } = runtimeForPath(
      "notes.txt",
      "root-a\0notes.txt\0r1\0fn main() {}",
    );

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

    resolvePython?.({
      get loadPythonEditorLanguage() {
        resolvePythonLanguageRead?.();
        queueMicrotask(() => {
          queueMicrotask(() => resolveStaleContinuation?.());
        });
        return () => [];
      },
    });
    await pythonLanguageRead;
    await staleContinuationSettled;

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

describe("CodeMirrorEditorRuntime controlled identity", () => {
  it("does not stringify the document for unrelated controlled updates", () => {
    const { runtime, view } = runtimeForPath(
      "notes.txt",
      "root-a\0notes.txt\0r1\0fn main() {}",
    );
    const toString = vi.spyOn(view.state.doc, "toString");
    toString.mockClear();

    runtime.update({
      ariaLabel: "Edit notes.txt (renamed label)",
      documentIdentity: "root-a\0notes.txt\0r1\0fn main() {}",
      path: "notes.txt",
      content: "fn main() {}",
      disabled: true,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });

    expect(toString).not.toHaveBeenCalled();
    runtime.destroy();
  });

  it("reconciles a changed authoritative identity, including same text at another resource", () => {
    const { runtime, view } = runtimeForPath("notes.txt");
    runtime.update({
      ariaLabel: "Edit notes.txt",
      documentIdentity: "root-a\0notes.txt\0r2\0restored",
      path: "notes.txt",
      content: "restored",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    expect(view.state.doc.toString()).toBe("restored");
    runtime.update({
      ariaLabel: "Edit copied.txt",
      documentIdentity: "root-a\0copied.txt\0r2\0restored",
      path: "copied.txt",
      content: "restored",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    expect(view.state.doc.toString()).toBe("restored");
    runtime.destroy();
  });

  it("does not let matching resource revision and fingerprint mask changed content", () => {
    const { runtime, view } = runtimeForPath(
      "notes.txt",
      "root-a\0notes.txt\0r1\0fingerprint-a",
    );
    runtime.update({
      ariaLabel: "Edit notes.txt",
      documentIdentity: "root-a\0notes.txt\0r1\0fingerprint-a",
      path: "notes.txt",
      content: "authoritative reload disagreement",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    expect(view.state.doc.toString()).toBe("authoritative reload disagreement");
    runtime.destroy();
  });

  it("keeps local undo through a completed save echo and accepts an external reload", () => {
    const { runtime, view } = runtimeForPath(
      "notes.txt",
      "root-a\0notes.txt\0r1\0fn main() {}",
    );
    view.dispatch({ changes: { from: view.state.doc.length, insert: "!" } });
    expect(view.state.doc.toString()).toBe("fn main() {}!");

    runtime.update({
      ariaLabel: "Edit notes.txt",
      documentIdentity: "root-a\0notes.txt\0r2\0fn main() {}!",
      path: "notes.txt",
      content: "fn main() {}!",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("fn main() {}");

    runtime.update({
      ariaLabel: "Edit notes.txt",
      documentIdentity: "root-a\0notes.txt\0r3\0reloaded from disk",
      path: "notes.txt",
      content: "reloaded from disk",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    });
    expect(view.state.doc.toString()).toBe("reloaded from disk");
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
