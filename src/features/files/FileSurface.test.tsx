import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useEditorCaretStore } from "../../app/editorCaretStore";
import { useWorkbenchStore } from "../../app/store";
import type { EditorDocument, WorkbenchTab } from "../../domain/workbench";
import { FIND_BAR_BINDINGS } from "../../domain/settingsNavigation";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { contentText } from "./editorContent";
import { FileSurface } from "./FileSurface";
import { findMatches, type FindRequest } from "./inFileFind";
import { MAX_EDITABLE_CONTENT_BYTES } from "./editorContent";

const path = "debounce.txt";
const resource = { rootId: "root-a", relativePath: path };
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
const tab: WorkbenchTab = {
  id: `file:${path}`,
  kind: "file",
  title: path,
  position: 1,
  pinned: false,
  dirty: false,
  resource,
};
const initialState = useWorkbenchStore.getState();
const initialCaret = useEditorCaretStore.getState();
const editorPort = getWorkbenchPorts().editor;

function viewForEditor(editor: HTMLElement) {
  const view = EditorView.findFromDOM(editor);
  if (!view) {
    throw new Error("Expected a mounted CodeMirror editor");
  }
  return view;
}

/** The store holds the editor's document after an edit, not a string. */
function activeEditorText() {
  const editor = useWorkbenchStore.getState().activeEditor;
  return editor?.phase === "ready" ? contentText(editor.content) : null;
}

function replaceEditorContent(editor: HTMLElement, content: string) {
  const view = viewForEditor(editor);
  act(() => {
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: content },
    });
  });
}

function editorSelection(editor: HTMLElement) {
  const selection = viewForEditor(editor).state.selection.main;
  return { start: selection.from, end: selection.to };
}

function setReadyEditor(
  nextTab: WorkbenchTab = tab,
  content = document.buffer.content,
) {
  const nextResource = nextTab.resource!;
  const nextDocument = {
    ...document,
    resource: nextResource,
    buffer: { ...document.buffer, content },
  };
  useWorkbenchStore.setState({
    tabs: [nextTab],
    activeTabId: nextTab.id,
    activeEditor: {
      phase: "ready",
      resource: nextResource,
      document: nextDocument,
      content,
      revision: 0,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    },
  });
}

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  useEditorCaretStore.setState(initialCaret, true);
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("FileSurface document language", () => {
  it("keeps one resource root across loading, error, and ready states", () => {
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      activeEditor: { phase: "loading", resource },
      loadEditor: vi.fn(async () => undefined),
    });
    render(<FileSurface tab={tab} />);

    const fileRegion = () =>
      screen.getByRole("region", { name: `File editor ${path}` });
    expect(fileRegion()).toHaveClass("file-surface", "resource-document");
    expect(screen.getByRole("status")).toHaveTextContent(`Loading ${path}…`);

    act(() => {
      useWorkbenchStore.setState({
        activeEditor: {
          phase: "error",
          resource,
          message: "The file is unavailable.",
        },
      });
    });
    expect(fileRegion()).toHaveClass("file-surface", "resource-document");
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(
      `Could not open ${path}`,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The file is unavailable.",
    );

    act(() => setReadyEditor());
    expect(fileRegion()).toHaveClass("file-surface", "resource-document");
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("FileSurface durability cadence", () => {
  it("keeps edit, keyboard save, undo, and conflict behavior at the File surface", async () => {
    const save = vi.fn(async () => true);
    setReadyEditor({ ...tab, dirty: true });
    useWorkbenchStore.setState((state) => ({
      saveActiveEditor: save,
      activeEditor:
        state.activeEditor?.phase === "ready"
          ? {
              ...state.activeEditor,
              document: {
                ...state.activeEditor.document,
                conflict: {
                  kind: "external-change",
                  message: "Disk content changed",
                },
              },
            }
          : state.activeEditor,
    }));
    render(<FileSurface tab={{ ...tab, dirty: true }} />);

    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    replaceEditorContent(editor, "edited\n");
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      revision: 1,
    });
    expect(activeEditorText()).toBe("edited\n");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "External change detected. Disk content changed",
    );

    fireEvent.keyDown(
      screen.getByRole("region", { name: `File editor ${path}` }),
      { key: "s", ctrlKey: true },
    );
    expect(save).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: "Discard" })).toBeNull();
  });

  it("keeps typing in the editor after a keyboard save", async () => {
    setReadyEditor({ ...tab, dirty: true });
    render(<FileSurface tab={{ ...tab, dirty: true }} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    const view = viewForEditor(editor);
    act(() => view.focus());
    const setSaving = (saving: boolean) =>
      act(() =>
        useWorkbenchStore.setState((state) => ({
          activeEditor:
            state.activeEditor?.phase === "ready"
              ? { ...state.activeEditor, saving }
              : state.activeEditor,
        })),
      );
    setSaving(true);
    // A browser drops focus from content that stops being editable.
    act(() => view.contentDOM.blur());
    setSaving(false);
    expect(view.hasFocus).toBe(true);
  });

  it("says why an edit past the size limit was not applied", async () => {
    setReadyEditor();
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    const view = viewForEditor(editor);
    act(() => {
      view.dispatch({
        changes: {
          from: 0,
          insert: "x".repeat(MAX_EDITABLE_CONTENT_BYTES),
        },
      });
    });
    expect(view.state.doc.toString()).toBe("base\n");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That edit was not applied: it would make this File larger than 1 MiB",
    );
    act(() => {
      view.dispatch({ changes: { from: 0, insert: "y" } });
    });
    expect(screen.queryByText(/That edit was not applied/u)).toBeNull();
  });

  it("focuses the editor of a File that New File just opened", async () => {
    const { requestEditorFocus } = await import("./editorFocusRequest");
    setReadyEditor();
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    expect(viewForEditor(editor).hasFocus).toBe(false);
    act(() =>
      requestEditorFocus({ rootId: "root-a", relativePath: "other.txt" }),
    );
    expect(viewForEditor(editor).hasFocus).toBe(false);
    act(() => requestEditorFocus({ ...resource }));
    await waitFor(() => expect(viewForEditor(editor).hasFocus).toBe(true));
  });

  it("takes a New File focus request made before its editor mounts", async () => {
    const { requestEditorFocus } = await import("./editorFocusRequest");
    requestEditorFocus(resource);
    setReadyEditor();
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    await waitFor(() => expect(viewForEditor(editor).hasFocus).toBe(true));
  });

  it("clears the dirty marker when undo returns a clean file to disk content", async () => {
    setReadyEditor();
    render(<FileSurface tab={tab} />);

    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    replaceEditorContent(editor, "edited\n");
    expect(useWorkbenchStore.getState().tabs[0]?.dirty).toBe(true);

    fireEvent.keyDown(editor, { key: "z", ctrlKey: true });

    expect(activeEditorText()).toBe(document.buffer.content);
    expect(useWorkbenchStore.getState().tabs[0]?.dirty).toBe(false);
  });

  it("sends one trailing snapshot after 750 ms", async () => {
    vi.useFakeTimers();
    const snapshot = vi
      .spyOn(editorPort, "snapshotEditorBuffer")
      .mockResolvedValue({ revision: 1, persistedAt: 1, dirty: true });
    useWorkbenchStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      activeEditor: {
        phase: "ready",
        resource,
        document,
        content: document.buffer.content,
        revision: 0,
        persistedRevision: 0,
        persisting: false,
        saving: false,
        message: null,
      },
    });
    render(<FileSurface tab={tab} />);

    act(() => {
      useWorkbenchStore.getState().updateEditorContent("edited\n");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(749);
    });
    expect(snapshot).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(snapshot).toHaveBeenCalledTimes(1);
    expect(snapshot).toHaveBeenCalledWith(
      expect.objectContaining({ resource, content: "edited\n", revision: 1 }),
    );
  });

  it("renders a file read-only when the host owns its review surface", async () => {
    // Evaluated up front, so an editing load would land before the check below.
    await import("./editorEditing");
    setReadyEditor();
    render(<FileSurface tab={tab} readOnly />);

    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    expect(editor).toHaveAttribute("aria-readonly", "true");
    replaceEditorContent(editor, "must not be accepted");
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: document.buffer.content,
      revision: 0,
    });

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    expect(
      await screen.findByRole("textbox", { name: "Find text" }),
    ).toBeVisible();
    fireEvent.keyDown(editor, { key: "g", ctrlKey: true });
    expect(screen.getByRole("textbox", { name: "Go to line" })).toBeVisible();
    fireEvent.keyDown(editor, { key: "s", ctrlKey: true });
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      content: document.buffer.content,
      revision: 0,
    });
    // The Agent artifact pane's read-only Files get no editing features
    // (EP-03): no fold gutter, no multiple cursors.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(
      editor.closest(".cm-editor")?.querySelector(".cm-foldGutter"),
    ).toBeNull();
  });

  it("gives an editable File its editing features within the same wait", async () => {
    await import("./editorEditing");
    setReadyEditor();
    render(<FileSurface tab={tab} />);

    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(
      editor.closest(".cm-editor")?.querySelector(".cm-foldGutter"),
    ).not.toBeNull();
  });

  it("keeps a read-only File free of editing features during a Project transition", async () => {
    await import("./editorEditing");
    setReadyEditor();
    useWorkbenchStore.setState({ projectTransitioning: true });
    render(<FileSurface tab={tab} readOnly />);

    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(
      editor.closest(".cm-editor")?.querySelector(".cm-foldGutter"),
    ).toBeNull();
  });

  it("freezes editor input while a Project transition owns the projection", async () => {
    setReadyEditor();
    useWorkbenchStore.setState({ projectTransitioning: true });
    render(<FileSurface tab={tab} />);

    expect(
      await screen.findByRole("textbox", { name: `Edit ${path}` }),
    ).toHaveAttribute("aria-readonly", "true");
    act(() => {
      useWorkbenchStore.getState().updateEditorContent("must not be accepted");
    });
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: document.buffer.content,
      revision: 0,
    });
  });

  it("isolates editor history and lifecycle by full Resource identity", async () => {
    const firstResource = {
      rootId: "root-a",
      relativePath: "src/shared.ts",
    };
    const firstTab = {
      ...tab,
      id: "file:root-a:src/shared.ts",
      title: "shared.ts",
      resource: firstResource,
    };
    setReadyEditor(firstTab, "first resource");
    const { rerender } = render(<FileSurface tab={firstTab} />);
    const firstEditor = await screen.findByRole("textbox", {
      name: "Edit src/shared.ts",
    });
    const firstView = viewForEditor(firstEditor);
    const destroy = vi.spyOn(firstView, "destroy");

    replaceEditorContent(firstEditor, "shared content");
    expect(firstView.state.doc.toString()).toBe("shared content");

    const secondResource = {
      rootId: "root-b",
      relativePath: "src/shared.ts",
    };
    const secondTab = {
      ...tab,
      id: "file:root-b:src/shared.ts",
      title: "shared.ts",
      resource: secondResource,
    };
    act(() => {
      setReadyEditor(secondTab, "shared content");
      rerender(<FileSurface tab={secondTab} />);
    });

    expect(destroy).toHaveBeenCalledOnce();
    const secondEditor = await screen.findByRole("textbox", {
      name: "Edit src/shared.ts",
    });
    const secondView = viewForEditor(secondEditor);
    expect(secondView).not.toBe(firstView);

    fireEvent.keyDown(secondEditor, { key: "z", ctrlKey: true });

    expect(secondView.state.doc.toString()).toBe("shared content");
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      resource: secondResource,
      content: "shared content",
      revision: 0,
    });
  });
});

describe("FileSurface location navigation", () => {
  it("places a collapsed caret at an exact Unicode-aware location", async () => {
    const content = "zero\n🙂 target";
    setReadyEditor(tab, content);
    useWorkbenchStore.setState({
      projectGeneration: 3,
      nativeTransitionEpoch: 7,
      fileNavigationTarget: {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 7,
        resource,
        lineNumber: 2,
        columnNumber: 3,
        selection: { kind: "caret" },
      },
    });
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    expect(editor).toHaveFocus();
    expect(editorSelection(editor)).toEqual({ start: 8, end: 8 });
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();
  });

  it("selects the exact Unicode match range and clamps stale offsets", async () => {
    const content = "zero\n🙂 needle tail";
    setReadyEditor(tab, content);
    useWorkbenchStore.setState({
      projectGeneration: 3,
      nativeTransitionEpoch: 7,
      fileNavigationTarget: {
        projectId: "project-a",
        projectGeneration: 3,
        transitionEpoch: 7,
        resource,
        lineNumber: 2,
        columnNumber: 3,
        selection: {
          kind: "match",
          query: "needle",
          caseSensitive: true,
        },
      },
    });
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    expect(editor).toHaveFocus();
    expect(editorSelection(editor)).toEqual({ start: 8, end: 14 });
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();

    act(() => {
      useWorkbenchStore.setState({
        fileNavigationTarget: {
          projectId: "project-a",
          projectGeneration: 3,
          transitionEpoch: 7,
          resource,
          lineNumber: 0xffff_ffff,
          columnNumber: 0xffff_ffff,
          selection: {
            kind: "match",
            query: "needle",
            caseSensitive: false,
          },
        },
      });
    });

    expect(editorSelection(editor)).toEqual({
      start: content.length,
      end: content.length,
    });
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();
  });

  it("does not reveal a Search target during a transition or after its epoch retires", async () => {
    setReadyEditor(tab, "alpha needle omega");
    const target = {
      projectId: "project-a",
      projectGeneration: 3,
      transitionEpoch: 7,
      resource,
      lineNumber: 1,
      columnNumber: 7,
      selection: {
        kind: "match" as const,
        query: "needle",
        caseSensitive: true,
      },
    };
    useWorkbenchStore.setState({
      projectGeneration: 3,
      nativeTransitionEpoch: 7,
      projectTransitioning: true,
      fileNavigationTarget: target,
    });
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    expect(editorSelection(editor).start).toBe(0);
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBe(target);

    act(() => {
      useWorkbenchStore.setState({
        nativeTransitionEpoch: 8,
        projectTransitioning: false,
      });
    });

    expect(editorSelection(editor).start).toBe(0);
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBe(target);
  });

  it("never selects content beyond the bounded editor ceiling", async () => {
    const content = `${"a".repeat(MAX_EDITABLE_CONTENT_BYTES)}\nneedle`;
    setReadyEditor(tab, content);
    useWorkbenchStore.setState({
      fileNavigationTarget: {
        projectId: "project-a",
        projectGeneration: useWorkbenchStore.getState().projectGeneration,
        transitionEpoch: useWorkbenchStore.getState().nativeTransitionEpoch,
        resource,
        lineNumber: 1,
        columnNumber: 0xffff_ffff,
        selection: {
          kind: "match",
          query: "needle",
          caseSensitive: true,
        },
      },
    });
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    expect(editorSelection(editor)).toEqual({
      start: MAX_EDITABLE_CONTENT_BYTES,
      end: MAX_EDITABLE_CONTENT_BYTES,
    });
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();
  });

  it("preserves an open Editor Find while revealing a Search match", async () => {
    setReadyEditor(tab, "alpha needle omega");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const find = await screen.findByRole("textbox", { name: "Find text" });
    fireEvent.change(find, { target: { value: "alpha" } });

    act(() => {
      useWorkbenchStore.setState({
        fileNavigationTarget: {
          projectId: "project-a",
          projectGeneration: useWorkbenchStore.getState().projectGeneration,
          transitionEpoch: 0,
          resource,
          lineNumber: 1,
          columnNumber: 7,
          selection: {
            kind: "match",
            query: "needle",
            caseSensitive: true,
          },
        },
      });
    });

    expect(screen.getByRole("search", { name: "Find in file" })).toBeVisible();
    expect(find).toHaveValue("alpha");
    expect(editor).toHaveFocus();
    expect(editorSelection(editor)).toEqual({ start: 6, end: 12 });
  });
});

describe("FileSurface in-file find", () => {
  it("navigates literal matches with wrap, case control, and editor focus recovery", async () => {
    setReadyEditor(tab, "Needle needle needle");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    editor.focus();
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true, shiftKey: true });
    expect(screen.queryByRole("search", { name: "Find in file" })).toBeNull();
    expect(editor).toHaveFocus();

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const query = await screen.findByRole("textbox", { name: "Find text" });
    expect(screen.getByRole("search", { name: "Find in file" })).toHaveClass(
      "editor-find--overlay",
    );
    expect(screen.getByRole("textbox", { name: `Edit ${path}` })).toBe(editor);
    expect(query).toHaveFocus();
    // The bar remembers its last query for the session, as VS Code does.
    fireEvent.change(query, { target: { value: "" } });
    expect(screen.getByRole("status")).toHaveTextContent("No query");

    fireEvent.change(query, { target: { value: "needle" } });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");
    expect(editorSelection(editor)).toEqual({ start: 0, end: 6 });

    fireEvent.click(screen.getByRole("button", { name: "Previous match" }));
    expect(screen.getByRole("status")).toHaveTextContent("3 of 3 matches");
    expect(editorSelection(editor).start).toBe(14);

    fireEvent.click(screen.getByRole("button", { name: "Next match" }));
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");
    expect(editorSelection(editor).start).toBe(0);

    fireEvent.keyDown(query, { key: "Enter", shiftKey: true });
    expect(screen.getByRole("status")).toHaveTextContent("3 of 3 matches");
    expect(editorSelection(editor).start).toBe(14);

    fireEvent.keyDown(query, { key: "Enter" });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");
    expect(editorSelection(editor).start).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "Match case" }));
    expect(screen.getByRole("button", { name: "Match case" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editorSelection(editor).start).toBe(7);

    fireEvent.change(query, { target: { value: "absent" } });
    expect(screen.getByRole("status")).toHaveTextContent("No matches");
    expect(editorSelection(editor)).toEqual({ start: 7, end: 7 });

    fireEvent.keyDown(query, { key: "Escape" });
    expect(screen.queryByRole("search", { name: "Find in file" })).toBeNull();
    expect(screen.getByRole("textbox", { name: `Edit ${path}` })).toBe(editor);
    expect(editor).toHaveFocus();
  });

  it("reports no match and saves while Find owns focus", async () => {
    const save = vi.fn(async () => true);
    setReadyEditor();
    useWorkbenchStore.setState({ saveActiveEditor: save });
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const query = await screen.findByRole("textbox", { name: "Find text" });
    expect(query).toHaveFocus();
    fireEvent.change(query, {
      target: { value: "absent" },
    });
    expect(screen.getByRole("status")).toHaveTextContent("No matches");

    fireEvent.keyDown(query, { key: "s", ctrlKey: true });
    expect(save).toHaveBeenCalledOnce();
  });

  it("fails closed when content exceeds the editor ceiling", async () => {
    setReadyEditor(tab, "a".repeat(MAX_EDITABLE_CONTENT_BYTES + 1));
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    expect(editor).toHaveAttribute(
      "data-max-content-bytes",
      String(MAX_EDITABLE_CONTENT_BYTES),
    );
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    fireEvent.change(
      await screen.findByRole("textbox", { name: "Find text" }),
      { target: { value: "a" } },
    );

    expect(screen.getByRole("status")).toHaveTextContent(
      "File exceeds the 1 MiB Find limit",
    );
    expect(
      screen.getByRole("button", { name: "Previous match" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next match" })).toBeDisabled();
  });

  it("recomputes results for content changes and retires the index for a new resource", async () => {
    setReadyEditor(tab, "alpha alpha");
    const { rerender } = render(<FileSurface tab={tab} />);
    let editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const query = await screen.findByRole("textbox", { name: "Find text" });
    fireEvent.change(query, { target: { value: "alpha" } });
    fireEvent.keyDown(query, { key: "Enter" });
    expect(screen.getByRole("status")).toHaveTextContent("2 of 2 matches");

    // Edits search again after a short pause, so typing never pays for it.
    replaceEditorContent(editor, "alpha");
    expect(screen.getByRole("status")).toHaveTextContent("2 of 2 matches");
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("1 of 1 match"),
    );

    const nextResource = { rootId: "root-b", relativePath: "next.txt" };
    const nextTab = {
      ...tab,
      id: "file:next.txt",
      title: "next.txt",
      resource: nextResource,
    };
    act(() => setReadyEditor(nextTab, "alpha z alpha"));
    rerender(<FileSurface tab={nextTab} />);
    editor = await screen.findByRole("textbox", { name: "Edit next.txt" });

    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editorSelection(editor)).toEqual({ start: 0, end: 5 });
  });
});

/** jsdom has no Worker; this one answers on the next microtask. */
class InlineFindWorker {
  onmessage: ((event: MessageEvent) => void) | null = null;
  postMessage(request: FindRequest) {
    queueMicrotask(() =>
      this.onmessage?.({ data: findMatches(request) } as MessageEvent),
    );
  }
  terminate() {}
}

async function openFindBar(
  editor: HTMLElement,
  chord: { key: string; ctrlKey?: boolean; metaKey?: boolean } = {
    key: "f",
    ctrlKey: true,
  },
) {
  fireEvent.keyDown(editor, chord);
  const query = await screen.findByRole("textbox", { name: "Find text" });
  fireEvent.change(query, { target: { value: "" } });
  for (const name of [
    "Match case",
    "Match whole word",
    "Use regular expression",
  ]) {
    const toggle = screen.getByRole("button", { name });
    if (toggle.getAttribute("aria-pressed") === "true") fireEvent.click(toggle);
  }
  return query;
}

describe("FileSurface Find and Replace", () => {
  it("replaces one match, then all, and one undo reverses Replace all", async () => {
    setReadyEditor(tab, "cat cat cat");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    const view = viewForEditor(editor);

    fireEvent.keyDown(await openFindBar(editor), { key: "Escape" });
    fireEvent.keyDown(editor, { key: "h", ctrlKey: true });
    await screen.findByRole("textbox", { name: "Replace text" });
    const query = screen.getByRole("textbox", { name: "Find text" });
    // With no query yet, Replace starts in the find box, as in VS Code.
    expect(query).toHaveFocus();
    expect(
      screen.getByRole("button", { name: "Toggle Replace" }),
    ).toHaveAttribute("aria-expanded", "true");
    fireEvent.change(query, { target: { value: "cat" } });
    fireEvent.keyDown(query, { key: "Escape" });
    fireEvent.keyDown(editor, { key: "h", ctrlKey: true });
    // With a query, it starts in the replace box.
    await waitFor(() =>
      expect(
        screen.getByRole("textbox", { name: "Replace text" }),
      ).toHaveFocus(),
    );
    fireEvent.change(screen.getByRole("textbox", { name: "Replace text" }), {
      target: { value: "dog" },
    });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");

    await act(async () => {
      fireEvent.keyDown(screen.getByRole("textbox", { name: "Replace text" }), {
        key: "Enter",
      });
    });
    expect(activeEditorText()).toBe("dog cat cat");
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editorSelection(editor)).toEqual({ start: 4, end: 7 });

    // A typed edit stays its own undo step, apart from Replace all.
    act(() => {
      view.dispatch({
        changes: { from: 11, insert: "!" },
        userEvent: "input.type",
      });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
    });
    expect(activeEditorText()).toBe("dog dog dog!");
    expect(screen.getByRole("status")).toHaveTextContent("No matches");
    // Typing right after it does not join the Replace all step either.
    act(() => {
      view.dispatch({
        changes: { from: 11, insert: "?" },
        userEvent: "input.type",
      });
    });

    act(() => {
      undo(view);
    });
    expect(activeEditorText()).toBe("dog dog dog!");
    act(() => {
      undo(view);
    });
    expect(activeEditorText()).toBe("dog cat cat!");
  });

  it("only selects the next match when Replace finds no match selected, as VS Code does", async () => {
    setReadyEditor(tab, "cat cat");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    const query = await openFindBar(editor);
    fireEvent.change(query, { target: { value: "cat" } });
    fireEvent.click(screen.getByRole("button", { name: "Toggle Replace" }));
    const replacement = screen.getByRole("textbox", { name: "Replace text" });
    fireEvent.change(replacement, { target: { value: "dog" } });
    act(() => {
      viewForEditor(editor).dispatch({ selection: { anchor: 2 } });
    });

    await act(async () => {
      fireEvent.keyDown(replacement, { key: "Enter" });
    });
    expect(activeEditorText()).toBe("cat cat");
    expect(editorSelection(editor)).toEqual({ start: 4, end: 7 });
    await act(async () => {
      fireEvent.keyDown(replacement, { key: "Enter" });
    });
    expect(activeEditorText()).toBe("cat dog");
  });

  it("selects the next match after a longer replacement and replaces past the match cap", async () => {
    setReadyEditor(tab, "foo foo foo");
    const { unmount } = render(<FileSurface tab={tab} />);
    let editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    let query = await openFindBar(editor);
    fireEvent.change(query, { target: { value: "foo" } });
    fireEvent.click(screen.getByRole("button", { name: "Toggle Replace" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Replace text" }), {
      target: { value: "barbaz" },
    });
    const replaceOne = async () =>
      act(async () => {
        fireEvent.keyDown(
          screen.getByRole("textbox", { name: "Replace text" }),
          { key: "Enter" },
        );
      });
    await replaceOne();
    expect(activeEditorText()).toBe("barbaz foo foo");
    expect(editorSelection(editor)).toEqual({ start: 7, end: 10 });
    await replaceOne();
    expect(activeEditorText()).toBe("barbaz barbaz foo");
    unmount();

    // Replace all changes every match, not only the 10,000 Find shows.
    setReadyEditor(tab, "a ".repeat(12_000));
    render(<FileSurface tab={tab} />);
    editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    query = await openFindBar(editor);
    fireEvent.change(query, { target: { value: "a" } });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 10000+ matches");
    fireEvent.click(screen.getByRole("button", { name: "Toggle Replace" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Replace text" }), {
      target: { value: "b" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
    });
    expect(activeEditorText()).toBe("b ".repeat(12_000));
  });

  it("searches a large File in the worker", async () => {
    const spawned = vi.fn();
    vi.stubGlobal(
      "Worker",
      class extends InlineFindWorker {
        constructor() {
          super();
          spawned();
        }
      },
    );
    try {
      setReadyEditor(tab, `${"x".repeat(70_000)} needle needle`);
      render(<FileSurface tab={tab} />);
      const editor = await screen.findByRole("textbox", {
        name: `Edit ${path}`,
      });
      const query = await openFindBar(editor);
      fireEvent.change(query, { target: { value: "needle" } });
      await waitFor(() =>
        expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches"),
      );
      expect(spawned).toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("replaces with pattern groups in the worker and reports a bad pattern", async () => {
    vi.stubGlobal("Worker", InlineFindWorker);
    try {
      setReadyEditor(tab, "a=1\nb=2");
      render(<FileSurface tab={tab} />);
      const editor = await screen.findByRole("textbox", {
        name: `Edit ${path}`,
      });
      const query = await openFindBar(editor);
      fireEvent.click(
        screen.getByRole("button", { name: "Use regular expression" }),
      );
      fireEvent.change(query, { target: { value: "^(\\w)=(\\d)$" } });
      await waitFor(() =>
        expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches"),
      );

      fireEvent.click(screen.getByRole("button", { name: "Toggle Replace" }));
      fireEvent.change(screen.getByRole("textbox", { name: "Replace text" }), {
        target: { value: "$2:$1" },
      });
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "Replace all" }));
      });
      expect(activeEditorText()).toBe("1:a\n2:b");

      fireEvent.change(query, { target: { value: "(?i:a)" } });
      await waitFor(() =>
        expect(screen.getByRole("status")).toHaveTextContent(
          "Use Match case, not an inline flag",
        ),
      );
      expect(screen.getByRole("status")).not.toHaveTextContent("(?i:a)");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("seeds the query from a one-line selection and leaves the caret alone while typing", async () => {
    setReadyEditor(tab, "alpha beta alpha\nbeta");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    const view = viewForEditor(editor);
    fireEvent.keyDown(await openFindBar(editor), { key: "Escape" });
    act(() => {
      view.dispatch({ selection: { anchor: 6, head: 10 } });
    });

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const query = await screen.findByRole("textbox", { name: "Find text" });
    expect(query).toHaveValue("beta");
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editorSelection(editor)).toEqual({ start: 6, end: 10 });

    // Typing elsewhere while the bar is open never pulls the caret back.
    editor.focus();
    act(() => {
      view.dispatch({ selection: { anchor: 0 } });
      view.dispatch(view.state.replaceSelection("x"));
    });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editorSelection(editor)).toEqual({ start: 1, end: 1 });

    // F3 and Shift+F3 continue from the caret, as in VS Code.
    fireEvent.keyDown(editor, { key: "F3" });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editorSelection(editor)).toEqual({ start: 7, end: 11 });
    fireEvent.keyDown(editor, { key: "F3" });
    expect(screen.getByRole("status")).toHaveTextContent("2 of 2 matches");
    expect(editorSelection(editor)).toEqual({ start: 18, end: 22 });
    fireEvent.keyDown(editor, { key: "F3" });
    expect(editorSelection(editor)).toEqual({ start: 7, end: 11 });
    act(() => {
      view.dispatch({ selection: { anchor: 14 } });
    });
    fireEvent.keyDown(editor, { key: "F3", shiftKey: true });
    expect(editorSelection(editor)).toEqual({ start: 7, end: 11 });
    fireEvent.keyDown(editor, { key: "F3", shiftKey: true });
    expect(editorSelection(editor)).toEqual({ start: 18, end: 22 });
  });

  it("answers exactly the find bar chords the Keyboard settings page lists", async () => {
    for (const apple of [false, true]) {
      vi.spyOn(navigator, "platform", "get").mockReturnValue(
        apple ? "MacIntel" : "Win32",
      );
      setReadyEditor(tab, "Cat cat cat");
      const { unmount } = render(<FileSurface tab={tab} />);
      const editor = await screen.findByRole("textbox", {
        name: `Edit ${path}`,
      });
      const query = await openFindBar(editor, {
        key: "f",
        [apple ? "metaKey" : "ctrlKey"]: true,
      });
      fireEvent.change(query, { target: { value: "cat" } });
      for (const binding of FIND_BAR_BINDINGS) {
        const chord = { ...binding, ...(apple ? binding.apple : undefined) };
        const primary = apple ? "metaKey" : "ctrlKey";
        const event = {
          // Option turns letters into other characters on macOS.
          key: chord.key === "enter" ? "Enter" : apple ? "ç" : chord.key,
          code:
            chord.key === "enter" ? "Enter" : `Key${chord.key.toUpperCase()}`,
          altKey: chord.alt === "required",
          [primary]: chord.ctrlOrMeta === "required",
        };
        if (binding.id === "find.replaceAll") {
          fireEvent.click(
            screen.getByRole("button", { name: "Toggle Replace" }),
          );
          const replacement = screen.getByRole("textbox", {
            name: "Replace text",
          });
          fireEvent.change(replacement, { target: { value: "x" } });
          await act(async () => {
            fireEvent.keyDown(replacement, { ...event, shiftKey: true });
          });
          expect(activeEditorText(), `${binding.id} with Shift`).toBe(
            "x cat cat",
          );
          fireEvent.change(replacement, { target: { value: "y" } });
          await act(async () => {
            fireEvent.keyDown(replacement, event);
          });
          expect(activeEditorText(), binding.id).toBe("x y y");
          continue;
        }
        const toggle = screen.getByRole("button", { name: binding.label });
        const before = toggle.getAttribute("aria-pressed");
        fireEvent.keyDown(query, { ...event, shiftKey: true });
        expect(toggle, `${binding.id} with Shift`).toHaveAttribute(
          "aria-pressed",
          before,
        );
        fireEvent.keyDown(query, event);
        expect(toggle, binding.id).not.toHaveAttribute("aria-pressed", before);
        fireEvent.click(toggle);
      }
      unmount();
    }
  });

  it("toggles options from the keyboard and keeps Replace off in a read-only File", async () => {
    setReadyEditor(tab, "Cat cat");
    render(<FileSurface tab={tab} readOnly />);
    const editor = await screen.findByRole("textbox", { name: `Edit ${path}` });
    const query = await openFindBar(editor);
    fireEvent.change(query, { target: { value: "cat" } });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");

    fireEvent.keyDown(query, { key: "c", code: "KeyC", altKey: true });
    expect(screen.getByRole("button", { name: "Match case" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("status")).toHaveTextContent("1 of 1 match");

    fireEvent.click(screen.getByRole("button", { name: "Toggle Replace" }));
    expect(
      screen.getByRole("textbox", { name: "Replace text" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Replace all" })).toBeDisabled();
    expect(activeEditorText()).toBe("Cat cat");
    // Ctrl+H with a remembered query still focuses a field it can type in.
    fireEvent.keyDown(query, { key: "Escape" });
    fireEvent.keyDown(editor, { key: "h", ctrlKey: true });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Find text" })).toHaveFocus(),
    );
  });
});

describe("FileSurface caret status", () => {
  it("keeps Ln and Col while typing at the end of the File", async () => {
    setReadyEditor(tab, "one\ntwo");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    const view = viewForEditor(editor);
    act(() => {
      view.dispatch({ selection: { anchor: 7 } });
      view.dispatch(view.state.replaceSelection("!"));
    });
    // The caret now sits past the end of the text the last render saw; the
    // status must come from the edited document, not that render.
    expect(useEditorCaretStore.getState().caret).toEqual({
      line: 2,
      column: 5,
    });
  });
});

describe("FileSurface Go-to Line", () => {
  it("jumps the caret from Ctrl+G and keeps Find mutually exclusive", async () => {
    setReadyEditor(tab, "one\ntwo\nthree");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });

    fireEvent.keyDown(editor, { key: "g", ctrlKey: true });
    const input = screen.getByRole("textbox", { name: "Go to line" });
    expect(input).toHaveFocus();
    expect(screen.getByRole("search", { name: "Go to line" })).toBeVisible();

    fireEvent.change(input, { target: { value: "2:2" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(editorSelection(editor)).toEqual({ start: 5, end: 5 });
    expect(useEditorCaretStore.getState().caret).toEqual({
      line: 2,
      column: 2,
    });
    expect(editor).toHaveFocus();
    expect(screen.queryByRole("search", { name: "Go to line" })).toBeNull();

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    expect(screen.getByRole("search", { name: "Find in file" })).toBeVisible();
    fireEvent.keyDown(editor, { key: "g", ctrlKey: true });
    expect(screen.queryByRole("search", { name: "Find in file" })).toBeNull();
    expect(screen.getByRole("search", { name: "Go to line" })).toBeVisible();

    fireEvent.keyDown(screen.getByLabelText(`File editor ${path}`), {
      key: "Escape",
    });
    expect(screen.queryByRole("search", { name: "Go to line" })).toBeNull();
    expect(editor).toHaveFocus();
  });

  it("rejects junk without moving the caret and stays closed in Preview", async () => {
    const markdownTab: WorkbenchTab = {
      ...tab,
      id: "file:docs/notes.md",
      title: "notes.md",
      resource: { rootId: "root-a", relativePath: "docs/notes.md" },
    };
    setReadyEditor(tab, "alpha\nbeta");
    render(<FileSurface tab={tab} />);
    const editor = await screen.findByRole("textbox", {
      name: `Edit ${path}`,
    });
    fireEvent.keyDown(editor, { key: "g", ctrlKey: true });
    const input = screen.getByRole("textbox", { name: "Go to line" });
    fireEvent.change(input, { target: { value: "nope" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByRole("status")).toHaveTextContent("Enter a line");
    expect(editorSelection(editor)).toEqual({ start: 0, end: 0 });
    expect(screen.getByRole("search", { name: "Go to line" })).toBeVisible();

    cleanup();
    setReadyEditor(markdownTab, "# Title\n\nHello");
    render(<FileSurface tab={markdownTab} />);
    await screen.findByRole("textbox", { name: "Edit docs/notes.md" });
    fireEvent.click(screen.getByRole("button", { name: "Preview file" }));
    fireEvent.keyDown(screen.getByLabelText("File editor docs/notes.md"), {
      key: "g",
      ctrlKey: true,
    });
    expect(screen.queryByRole("search", { name: "Go to line" })).toBeNull();
  });
});

describe("FileSurface local symbol navigation", () => {
  const sourceTab: WorkbenchTab = {
    ...tab,
    id: "file:src/example.ts",
    title: "example.ts",
    resource: { rootId: "root-a", relativePath: "src/example.ts" },
  };

  it("moves the F12 caret to the declaration from the File's syntax Outline", async () => {
    const content =
      "const value = example();\n\nexport function example() {}\n";
    setReadyEditor(sourceTab, content);
    render(<FileSurface tab={sourceTab} />);
    const editor = await screen.findByRole("textbox", {
      name: "Edit src/example.ts",
    });
    act(() => viewForEditor(editor).dispatch({ selection: { anchor: 16 } }));

    fireEvent.keyDown(editor, { key: "F12" });
    const declaration = content.indexOf("example() {}");
    await waitFor(() =>
      expect(editorSelection(editor)).toEqual({
        start: declaration,
        end: declaration,
      }),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Moved to local symbol.",
    );
  });

  it("says so and leaves the caret when the File declares no such name", async () => {
    const content = "missing();\nconst other = 1;\n";
    setReadyEditor(sourceTab, content);
    render(<FileSurface tab={sourceTab} />);
    const editor = await screen.findByRole("textbox", {
      name: "Edit src/example.ts",
    });
    act(() => viewForEditor(editor).dispatch({ selection: { anchor: 2 } }));

    fireEvent.keyDown(editor, { key: "F12" });
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "No declaration in this File for that name.",
      ),
    );
    expect(editorSelection(editor)).toEqual({ start: 2, end: 2 });
  });
});

describe("FileSurface Markdown Preview", () => {
  const markdownTab: WorkbenchTab = {
    ...tab,
    id: "file:docs/readme.md",
    title: "readme.md",
    resource: { rootId: "root-a", relativePath: "docs/readme.md" },
  };

  it("opens Agent Markdown as rendered content and offers View source", async () => {
    setReadyEditor(markdownTab, "# Agent notes\n\nReady for review.");
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "Sage", roots: [] },
    });
    render(<FileSurface tab={markdownTab} readOnly />);

    expect(
      screen.getByRole("navigation", { name: "File path" }),
    ).toHaveTextContent("Sagedocsreadme.md");
    expect(screen.getByRole("heading", { name: "Agent notes" })).toBeVisible();
    expect(screen.getByText("Ready for review.")).toBeVisible();
    expect(
      screen.queryByRole("textbox", { name: "Edit docs/readme.md" }),
    ).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "View source" }));
    expect(
      await screen.findByRole("textbox", { name: "Edit docs/readme.md" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("article", { name: "Markdown preview" }),
    ).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "View preview" }));
    expect(screen.getByRole("heading", { name: "Agent notes" })).toBeVisible();
  });

  it("hides Preview on non-markdown files", async () => {
    setReadyEditor(tab, "plain");
    render(<FileSurface tab={tab} />);
    await screen.findByRole("textbox", { name: `Edit ${path}` });
    expect(screen.queryByRole("button", { name: "Preview file" })).toBeNull();
  });

  it("unmounts the editor while previewing live buffer markdown", async () => {
    setReadyEditor(
      markdownTab,
      "# Title\n\nHello **world** and [docs](https://example.test).",
    );
    render(<FileSurface tab={markdownTab} />);
    const editor = await screen.findByRole("textbox", {
      name: "Edit docs/readme.md",
    });

    fireEvent.click(screen.getByRole("button", { name: "Preview file" }));

    expect(
      screen.queryByRole("textbox", { name: "Edit docs/readme.md" }),
    ).toBeNull();
    expect(
      screen.getByRole("article", { name: "Markdown preview" }),
    ).toBeVisible();
    expect(screen.getByRole("heading", { name: "Title" })).toBeVisible();
    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName === "P" &&
          element.textContent === "Hello world and docs.",
      ),
    ).toBeVisible();
    expect(screen.getByText("world", { selector: "strong" })).toBeVisible();
    expect(screen.queryByText("https://example.test")).toBeNull();
    expect(
      screen.getByRole("button", { name: "Return to editor" }),
    ).toBeVisible();

    const surface = screen.getByLabelText("File editor docs/readme.md");
    fireEvent.keyDown(surface, { key: "f", ctrlKey: true });
    expect(screen.queryByRole("search", { name: "Find in file" })).toBeNull();
    fireEvent.keyDown(surface, { key: "Escape" });
    expect(
      await screen.findByRole("textbox", { name: "Edit docs/readme.md" }),
    ).toBeVisible();
    expect(editor).not.toBeInTheDocument();
  });

  it("draws mermaid fences in the live markdown preview", async () => {
    setReadyEditor(
      markdownTab,
      ["```mermaid", "graph TD", "A --> B", "```"].join("\n"),
    );
    render(<FileSurface tab={markdownTab} />);
    await screen.findByRole("textbox", { name: "Edit docs/readme.md" });
    fireEvent.click(screen.getByRole("button", { name: "Preview file" }));
    expect(
      await screen.findByRole("img", { name: "Mermaid diagram" }),
    ).toBeVisible();
    expect(
      await screen.findByRole("button", { name: "Copy diagram source" }),
    ).toBeVisible();
  });

  it("previews standalone mermaid files as diagrams", async () => {
    const mermaidTab: WorkbenchTab = {
      ...tab,
      id: "file:docs/flow.mmd",
      title: "flow.mmd",
      resource: { rootId: "root-a", relativePath: "docs/flow.mmd" },
    };
    setReadyEditor(mermaidTab, "graph TD\nA --> B");
    render(<FileSurface tab={mermaidTab} />);
    await screen.findByRole("textbox", { name: "Edit docs/flow.mmd" });
    fireEvent.click(screen.getByRole("button", { name: "Preview file" }));
    expect(
      screen.getByRole("article", { name: "Diagram preview" }),
    ).toBeVisible();
    expect(
      await screen.findByRole("img", { name: "Mermaid diagram" }),
    ).toBeVisible();
  });
});
