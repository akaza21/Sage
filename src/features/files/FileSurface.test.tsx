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
