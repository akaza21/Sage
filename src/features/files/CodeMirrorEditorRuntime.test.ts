import { undo } from "@codemirror/commands";
import { syntaxTree, type TagStyle } from "@codemirror/language";
import { EditorState, Text, type Extension } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
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
    expect(onCaretOffset).toHaveBeenCalledWith(0, expect.any(Text));
    runtime.setSelection(6, 6);
    expect(onCaretOffset).toHaveBeenLastCalledWith(6, expect.any(Text));
    runtime.destroy();
  });
});

describe("CodeMirrorEditorRuntime editing features", () => {
  const editing = {
    matchBrackets: true,
    autoClosingBrackets: true,
    folding: true,
    occurrencesHighlight: true,
    multiCursorModifier: "alt" as const,
  };

  function mountEditing(next: typeof editing | null) {
    const parent = document.createElement("div");
    document.body.append(parent);
    const props = {
      ariaLabel: "Edit main.ts",
      path: "main.ts",
      content: "function f() {\n  return 1;\n}\n",
      disabled: false,
      maxContentLength: 1024,
      onChange: vi.fn(),
    };
    const runtime = createCodeMirrorEditorRuntime({
      parent,
      ...props,
      editing: next,
    });
    const view = EditorView.findFromDOM(
      parent.querySelector<HTMLElement>(".cm-content")!,
    )!;
    return { runtime, view, props };
  }

  const allowsMultipleSelections = (view: EditorView) =>
    view.state.facet(EditorState.allowMultipleSelections);

  it("loads them on demand for an editable File", async () => {
    const { runtime, view } = mountEditing(editing);
    await vi.waitFor(() => expect(allowsMultipleSelections(view)).toBe(true));
    // Fold arrows sit between the line numbers and the text, as in VS Code.
    expect(
      [...view.dom.querySelectorAll(".cm-gutter")].map((gutter) =>
        gutter.classList.contains("cm-foldGutter") ? "fold" : "lineNumbers",
      ),
    ).toEqual(["lineNumbers", "fold"]);
    runtime.destroy();
  });

  it("puts the editing keys ahead of the base keymap", async () => {
    const { runtime, view } = mountEditing(editing);
    await vi.waitFor(() => expect(allowsMultipleSelections(view)).toBe(true));
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: '""' },
      selection: { anchor: 1 },
    });
    // The base keymap would delete one quote; auto-close deletes the pair.
    expect(
      runScopeHandlers(
        view,
        new KeyboardEvent("keydown", { key: "Backspace", keyCode: 8 }),
        "editor",
      ),
    ).toBe(true);
    expect(view.state.doc.toString()).toBe("");
    runtime.destroy();
  });

  it("gives a read-only view none of them", async () => {
    // Load the module first, so its absence is the runtime's choice.
    const loaded = mountEditing(editing);
    await vi.waitFor(() =>
      expect(allowsMultipleSelections(loaded.view)).toBe(true),
    );
    loaded.runtime.destroy();
    const { runtime, view } = mountEditing(null);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(allowsMultipleSelections(view)).toBe(false);
    expect(view.dom.querySelector(".cm-foldGutter")).toBeNull();
    runtime.destroy();
  });

  it("starts a later editor with them already in place", async () => {
    const first = mountEditing(editing);
    await vi.waitFor(() =>
      expect(allowsMultipleSelections(first.view)).toBe(true),
    );
    const second = mountEditing(editing);
    expect(allowsMultipleSelections(second.view)).toBe(true);
    first.runtime.destroy();
    second.runtime.destroy();
  });

  it("switches a feature off and on from Settings without remounting", async () => {
    const { runtime, view, props } = mountEditing(editing);
    await vi.waitFor(() =>
      expect(view.dom.querySelector(".cm-foldGutter")).not.toBeNull(),
    );
    view.dispatch({ selection: { anchor: 5 } });
    runtime.update({ ...props, editing: { ...editing, folding: false } });
    expect(view.dom.querySelector(".cm-foldGutter")).toBeNull();
    expect(view.state.selection.main.head).toBe(5);
    runtime.update({ ...props, editing: { ...editing } });
    expect(view.dom.querySelector(".cm-foldGutter")).not.toBeNull();
    runtime.update({ ...props, editing: null });
    expect(allowsMultipleSelections(view)).toBe(false);
    runtime.destroy();
  });

  it("does not reconfigure for an equal set of features", async () => {
    const { runtime, view, props } = mountEditing(editing);
    await vi.waitFor(() => expect(allowsMultipleSelections(view)).toBe(true));
    const dispatch = vi.spyOn(view, "dispatch");
    runtime.update({ ...props, editing: { ...editing } });
    expect(dispatch).not.toHaveBeenCalled();
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

// The effective colours and treatments of highlighted syntax are asserted
// in a real browser by tests/ui/editor-syntax.visual.ts.
describe("documentSyntaxHighlightStyle after a dependency drift", () => {
  it("keeps the editor usable and token-coloured when the default specs change", async () => {
    vi.resetModules();
    vi.doMock("@codemirror/language", async (importOriginal) => {
      const actual = (await importOriginal()) as Record<string, unknown> & {
        defaultHighlightStyle: { specs: readonly TagStyle[] };
      };
      const keyword = actual.defaultHighlightStyle.specs.find(
        (spec) => String(spec.tag) === "keyword",
      )!.tag;
      return {
        ...actual,
        // A new release that reorders and shrinks the default table, with
        // its own raw palette colour.
        defaultHighlightStyle: {
          specs: [{ tag: keyword, color: "#123456" }],
        },
      };
    });
    try {
      const { createCodeMirrorEditorRuntime: createDrifted } =
        await import("./CodeMirrorEditorRuntime");
      const parent = document.createElement("div");
      document.body.append(parent);
      const onChange = vi.fn();
      const runtime = createDrifted({
        parent,
        ariaLabel: "Edit main.ts",
        path: "main.ts",
        content: "const value = 1;\n",
        disabled: false,
        maxContentLength: 1024,
        onChange,
      });
      const content = parent.querySelector<HTMLElement>(".cm-content")!;
      expect(content.textContent).toBe("const value = 1;");

      const keyword = await vi.waitFor(() => {
        const span = [...content.querySelectorAll("span[class]")].find(
          (node) => node.textContent === "const",
        );
        expect(span).toBeDefined();
        return span!;
      });
      expect(getComputedStyle(keyword).color).toMatch(/^var\(--doc-syntax-/);

      // The view class of the same fresh module graph as the editor.
      const { EditorView: DriftedView } = await import("@codemirror/view");
      DriftedView.findFromDOM(content)!.dispatch({
        changes: { from: 0, to: 5, insert: "let" },
      });
      expect(content.textContent).toBe("let value = 1;");
      expect(String(onChange.mock.lastCall?.[0])).toBe("let value = 1;\n");
      runtime.destroy();
    } finally {
      vi.doUnmock("@codemirror/language");
      vi.resetModules();
    }
  });
});

describe("CodeMirrorEditorRuntime focus across a save", () => {
  const props = (disabled: boolean) => ({
    ariaLabel: "Edit notes.md",
    path: "notes.md",
    content: "hello",
    disabled,
    maxContentLength: 1024,
    onChange: vi.fn(),
  });

  it("gives focus back when a save that disabled the focused editor ends", () => {
    const { runtime, view } = runtimeForPath("notes.md", undefined, "hello");
    view.focus();
    expect(view.hasFocus).toBe(true);
    runtime.update(props(true));
    // The browser drops focus from content that stops being editable.
    view.contentDOM.blur();
    expect(view.hasFocus).toBe(false);
    runtime.update(props(false));
    expect(view.hasFocus).toBe(true);
  });

  it("leaves focus alone when the user moved it or the editor was not focused", () => {
    const { runtime, view } = runtimeForPath("notes.md", undefined, "hello");
    const elsewhere = document.createElement("button");
    document.body.append(elsewhere);
    view.focus();
    runtime.update(props(true));
    elsewhere.focus();
    runtime.update(props(false));
    expect(document.activeElement).toBe(elsewhere);

    elsewhere.blur();
    runtime.update(props(true));
    runtime.update(props(false));
    expect(view.hasFocus).toBe(false);
    elsewhere.remove();
  });

  it("leaves focus on the page after a click elsewhere that does not take it", () => {
    const { runtime, view } = runtimeForPath("notes.md", undefined, "hello");
    const elsewhere = document.createElement("button");
    document.body.append(elsewhere);
    view.focus();
    runtime.update(props(true));
    view.contentDOM.blur();
    // WKWebView leaves focus on the page when a button is clicked.
    elsewhere.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    runtime.update(props(false));
    expect(view.hasFocus).toBe(false);
    elsewhere.remove();
  });
});
