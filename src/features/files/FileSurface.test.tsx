import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { EditorDocument, WorkbenchTab } from "../../domain/workbench";
import { desktop } from "../../services/desktop";
import { FileSurface } from "./FileSurface";
import { MAX_EDITABLE_CONTENT_BYTES } from "./inFileFind";

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
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("FileSurface durability cadence", () => {
  it("keeps edit, save, discard, and conflict behavior at the File surface", () => {
    const save = vi.fn(async () => true);
    const discard = vi.fn(async () => true);
    setReadyEditor({ ...tab, dirty: true });
    useWorkbenchStore.setState((state) => ({
      saveActiveEditor: save,
      discardActiveEditor: discard,
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

    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });
    fireEvent.change(editor, { target: { value: "edited\n" } });
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: "edited\n",
      revision: 1,
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "External change detected. Disk content changed",
    );

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(save).toHaveBeenCalledOnce();
    expect(discard).toHaveBeenCalledOnce();
  });

  it("sends one trailing snapshot after 750 ms", async () => {
    vi.useFakeTimers();
    const snapshot = vi
      .spyOn(desktop, "snapshotEditorBuffer")
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

  it("freezes editor input while a Project transition owns the projection", () => {
    setReadyEditor();
    useWorkbenchStore.setState({ projectTransitioning: true });
    render(<FileSurface tab={tab} />);

    expect(
      screen.getByRole("textbox", { name: `Edit ${path}` }),
    ).toBeDisabled();
    act(() => {
      useWorkbenchStore.getState().updateEditorContent("must not be accepted");
    });
    expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
      phase: "ready",
      content: document.buffer.content,
      revision: 0,
    });
  });
});

describe("FileSurface Search navigation", () => {
  it("selects the exact Unicode match range and clamps stale offsets", () => {
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
        query: "needle",
        caseSensitive: true,
      },
    });
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    expect(editor).toHaveFocus();
    expect(editor).toHaveProperty("selectionStart", 8);
    expect(editor).toHaveProperty("selectionEnd", 14);
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
          query: "needle",
          caseSensitive: false,
        },
      });
    });

    expect(editor).toHaveProperty("selectionStart", content.length);
    expect(editor).toHaveProperty("selectionEnd", content.length);
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();
  });

  it("does not reveal a Search target during a transition or after its epoch retires", () => {
    setReadyEditor(tab, "alpha needle omega");
    const target = {
      projectId: "project-a",
      projectGeneration: 3,
      transitionEpoch: 7,
      resource,
      lineNumber: 1,
      columnNumber: 7,
      query: "needle",
      caseSensitive: true,
    };
    useWorkbenchStore.setState({
      projectGeneration: 3,
      nativeTransitionEpoch: 7,
      projectTransitioning: true,
      fileNavigationTarget: target,
    });
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    expect(editor).toHaveProperty("selectionStart", 0);
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBe(target);

    act(() => {
      useWorkbenchStore.setState({
        nativeTransitionEpoch: 8,
        projectTransitioning: false,
      });
    });

    expect(editor).toHaveProperty("selectionStart", 0);
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBe(target);
  });

  it("never selects content beyond the bounded editor ceiling", () => {
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
        query: "needle",
        caseSensitive: true,
      },
    });
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    expect(editor).toHaveProperty("selectionStart", MAX_EDITABLE_CONTENT_BYTES);
    expect(editor).toHaveProperty("selectionEnd", MAX_EDITABLE_CONTENT_BYTES);
    expect(useWorkbenchStore.getState().fileNavigationTarget).toBeNull();
  });

  it("preserves an open Editor Find while revealing a Search match", () => {
    setReadyEditor(tab, "alpha needle omega");
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const find = screen.getByRole("textbox", { name: "Find text" });
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
          query: "needle",
          caseSensitive: true,
        },
      });
    });

    expect(screen.getByRole("search", { name: "Find in file" })).toBeVisible();
    expect(find).toHaveValue("alpha");
    expect(editor).toHaveFocus();
    expect(editor).toHaveProperty("selectionStart", 6);
    expect(editor).toHaveProperty("selectionEnd", 12);
  });
});

describe("FileSurface in-file find", () => {
  it("navigates literal matches with wrap, case control, and editor focus recovery", () => {
    setReadyEditor(tab, "Needle needle needle");
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    editor.focus();
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true, shiftKey: true });
    expect(screen.queryByRole("search", { name: "Find in file" })).toBeNull();
    expect(editor).toHaveFocus();

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const query = screen.getByRole("textbox", { name: "Find text" });
    expect(query).toHaveFocus();
    expect(screen.getByRole("status")).toHaveTextContent("No query");

    fireEvent.change(query, { target: { value: "needle" } });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");
    expect(editor).toHaveProperty("selectionStart", 0);
    expect(editor).toHaveProperty("selectionEnd", 6);

    fireEvent.click(screen.getByRole("button", { name: "Previous match" }));
    expect(screen.getByRole("status")).toHaveTextContent("3 of 3 matches");
    expect(editor).toHaveProperty("selectionStart", 14);

    fireEvent.click(screen.getByRole("button", { name: "Next match" }));
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");
    expect(editor).toHaveProperty("selectionStart", 0);

    fireEvent.keyDown(query, { key: "Enter", shiftKey: true });
    expect(screen.getByRole("status")).toHaveTextContent("3 of 3 matches");
    expect(editor).toHaveProperty("selectionStart", 14);

    fireEvent.keyDown(query, { key: "Enter" });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 3 matches");
    expect(editor).toHaveProperty("selectionStart", 0);

    fireEvent.click(screen.getByRole("button", { name: "Match case" }));
    expect(screen.getByRole("button", { name: "Match case" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editor).toHaveProperty("selectionStart", 7);

    fireEvent.change(query, { target: { value: "absent" } });
    expect(screen.getByRole("status")).toHaveTextContent("No matches");
    expect(editor).toHaveProperty("selectionStart", 7);
    expect(editor).toHaveProperty("selectionEnd", 7);

    fireEvent.keyDown(query, { key: "Escape" });
    expect(screen.queryByRole("search", { name: "Find in file" })).toBeNull();
    expect(editor).toHaveFocus();
  });

  it("reports no match and saves while Find owns focus", () => {
    const save = vi.fn(async () => true);
    setReadyEditor();
    useWorkbenchStore.setState({ saveActiveEditor: save });
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    fireEvent.keyDown(editor, { key: "f", metaKey: true });
    const query = screen.getByRole("textbox", { name: "Find text" });
    expect(query).toHaveFocus();
    fireEvent.change(query, {
      target: { value: "absent" },
    });
    expect(screen.getByRole("status")).toHaveTextContent("No matches");

    fireEvent.keyDown(query, { key: "s", ctrlKey: true });
    expect(save).toHaveBeenCalledOnce();
  });

  it("fails closed when content exceeds the editor ceiling", () => {
    setReadyEditor(tab, "a".repeat(MAX_EDITABLE_CONTENT_BYTES + 1));
    render(<FileSurface tab={tab} />);
    const editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    expect(editor).toHaveAttribute(
      "maxLength",
      String(MAX_EDITABLE_CONTENT_BYTES),
    );
    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    fireEvent.change(screen.getByRole("textbox", { name: "Find text" }), {
      target: { value: "a" },
    });

    expect(screen.getByRole("status")).toHaveTextContent(
      "File exceeds the 1 MiB Find limit",
    );
    expect(
      screen.getByRole("button", { name: "Previous match" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Next match" })).toBeDisabled();
  });

  it("recomputes results for content changes and retires the index for a new resource", () => {
    setReadyEditor(tab, "alpha alpha");
    const { rerender } = render(<FileSurface tab={tab} />);
    let editor = screen.getByRole("textbox", { name: `Edit ${path}` });

    fireEvent.keyDown(editor, { key: "f", ctrlKey: true });
    const query = screen.getByRole("textbox", { name: "Find text" });
    fireEvent.change(query, { target: { value: "alpha" } });
    fireEvent.keyDown(query, { key: "Enter" });
    expect(screen.getByRole("status")).toHaveTextContent("2 of 2 matches");

    fireEvent.change(editor, { target: { value: "alpha" } });
    expect(screen.getByRole("status")).toHaveTextContent("1 of 1 match");

    const nextResource = { rootId: "root-b", relativePath: "next.txt" };
    const nextTab = {
      ...tab,
      id: "file:next.txt",
      title: "next.txt",
      resource: nextResource,
    };
    act(() => setReadyEditor(nextTab, "alpha z alpha"));
    rerender(<FileSurface tab={nextTab} />);
    editor = screen.getByRole("textbox", { name: "Edit next.txt" });

    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 matches");
    expect(editor).toHaveProperty("selectionStart", 0);
    expect(editor).toHaveProperty("selectionEnd", 5);
  });
});
