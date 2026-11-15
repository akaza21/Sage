import { createRef } from "react";
import { EditorState, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";

const initialSettings = useEditorSettingsStore.getState();

afterEach(() => {
  cleanup();
  useEditorSettingsStore.setState(initialSettings, true);
});

function viewForEditor(editor: HTMLElement) {
  const view = EditorView.findFromDOM(editor);
  if (!view) {
    throw new Error("Expected a mounted CodeMirror editor");
  }
  return view;
}

describe("EditorAdapter", () => {
  it("mounts one accessible line-numbered syntax editor without a duplicate textarea", async () => {
    const handle = createRef<EditorAdapterHandle>();
    const { container } = render(
      <EditorAdapter
        ref={handle}
        ariaLabel="Edit example.ts"
        path="src/example.ts"
        content="const answer = 42;"
        disabled={false}
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );

    const editor = await screen.findByRole("textbox", {
      name: "Edit example.ts",
    });
    expect(screen.getAllByRole("textbox")).toHaveLength(1);
    expect(editor).toHaveAttribute("contenteditable", "true");
    expect(editor).toHaveAttribute("aria-multiline", "true");
    expect(container.querySelectorAll("textarea")).toHaveLength(0);
    expect(container.querySelector(".cm-lineNumbers")).toHaveTextContent("1");
    expect(container.querySelector(".cm-content")).toHaveTextContent(
      "const answer = 42;",
    );
    await waitFor(
      () => expect(container.querySelector(".cm-content span")).not.toBeNull(),
      { timeout: 5_000 },
    );

    handle.current?.focus();
    handle.current?.setSelection(6, 12);
    expect(editor).toHaveFocus();
    expect(handle.current?.selection()).toEqual({ start: 6, end: 12 });
  });

  it("gives an editable File the editing features and a read-only view none", async () => {
    const editable = render(
      <EditorAdapter
        ariaLabel="Edit main.ts"
        path="main.ts"
        content={"function f() {\n  return 1;\n}\n"}
        disabled={false}
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );
    await waitFor(() =>
      expect(editable.container.querySelector(".cm-foldGutter")).not.toBeNull(),
    );
    act(() => {
      useEditorSettingsStore.setState((state) => ({
        settings: { ...state.settings, "editor.folding": false },
      }));
    });
    expect(editable.container.querySelector(".cm-foldGutter")).toBeNull();
    editable.unmount();

    const readOnly = render(
      <EditorAdapter
        ariaLabel="Edit main.ts"
        path="main.ts"
        content={"function f() {\n  return 1;\n}\n"}
        disabled
        readOnly
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );
    const editor = await screen.findByRole("textbox", { name: "Edit main.ts" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(readOnly.container.querySelector(".cm-foldGutter")).toBeNull();
    expect(
      viewForEditor(editor).state.facet(EditorState.allowMultipleSelections),
    ).toBe(false);
  });

  it("reconciles controlled content and read-only state", async () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <EditorAdapter
        ariaLabel="Edit notes.txt"
        path="notes.txt"
        content="draft"
        disabled={false}
        maxContentLength={1024}
        onChange={onChange}
      />,
    );

    rerender(
      <EditorAdapter
        ariaLabel="Edit notes.txt"
        path="notes.txt"
        content="restored from durable truth"
        disabled
        maxContentLength={1024}
        onChange={onChange}
      />,
    );

    await waitFor(() =>
      expect(container.querySelector(".cm-content")).toHaveTextContent(
        "restored from durable truth",
      ),
    );
    expect(
      await screen.findByRole("textbox", { name: "Edit notes.txt" }),
    ).toHaveAttribute("aria-readonly", "true");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps a local selection through unrelated updates with the same document identity", async () => {
    const { rerender } = render(
      <EditorAdapter
        ariaLabel="Edit notes.txt"
        documentIdentity="root-a\0notes.txt\0r1\0draft"
        path="notes.txt"
        content="draft"
        disabled={false}
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );
    const editor = await screen.findByRole("textbox", {
      name: "Edit notes.txt",
    });
    const view = viewForEditor(editor);
    view.dispatch({ selection: { anchor: 1, head: 3 } });
    rerender(
      <EditorAdapter
        ariaLabel="Edit durable notes.txt"
        documentIdentity="root-a\0notes.txt\0r1\0draft"
        path="notes.txt"
        content="draft"
        disabled
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );
    expect(view.state.selection.main).toMatchObject({ from: 1, to: 3 });
  });

  it("projects the same display settings into every shared File editor", async () => {
    useEditorSettingsStore.setState({
      settings: {
        ...initialSettings.settings,
        "editor.lineNumbers": false,
        "editor.renderWhitespace": true,
      },
    });
    const { container } = render(
      <EditorAdapter
        ariaLabel="Edit shared.txt"
        path="shared.txt"
        content={"one two\tthree"}
        disabled={false}
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );

    await screen.findByRole("textbox", { name: "Edit shared.txt" });
    expect(container.querySelector(".cm-lineNumbers")).toBeNull();
    expect(container.querySelector(".cm-highlightSpace")).not.toBeNull();
    expect(container.querySelector(".cm-highlightTab")).not.toBeNull();
  });

  it("emits exact edits and supports keyboard undo and redo", async () => {
    const onChange = vi.fn();
    render(
      <EditorAdapter
        ariaLabel="Edit history.txt"
        path="history.txt"
        content="one"
        disabled={false}
        maxContentLength={1024}
        onChange={onChange}
      />,
    );
    const editor = await screen.findByRole("textbox", {
      name: "Edit history.txt",
    });
    const view = viewForEditor(editor);

    act(() => {
      view.dispatch({ changes: { from: 0, to: 3, insert: "two" } });
    });
    expect(view.state.doc.toString()).toBe("two");
    expect(String(onChange.mock.lastCall?.[0])).toBe("two");

    fireEvent.keyDown(editor, { key: "z", ctrlKey: true });
    expect(view.state.doc.toString()).toBe("one");
    expect(String(onChange.mock.lastCall?.[0])).toBe("one");

    fireEvent.keyDown(editor, { key: "y", ctrlKey: true });
    expect(view.state.doc.toString()).toBe("two");
    expect(String(onChange.mock.lastCall?.[0])).toBe("two");
  });

  it("preserves composition edits and rejects content beyond the UTF-8 byte ceiling", async () => {
    const onChange = vi.fn();
    render(
      <EditorAdapter
        ariaLabel="Edit composition.txt"
        path="composition.txt"
        content="ok"
        disabled={false}
        maxContentLength={8}
        onChange={onChange}
      />,
    );
    const editor = await screen.findByRole("textbox", {
      name: "Edit composition.txt",
    });
    const view = viewForEditor(editor);

    fireEvent.compositionStart(editor);
    act(() => {
      view.dispatch({
        changes: { from: 2, insert: "🙂" },
        annotations: Transaction.userEvent.of("input.type.compose"),
      });
    });
    fireEvent.compositionEnd(editor);
    expect(view.state.doc.toString()).toBe("ok🙂");
    expect(String(onChange.mock.lastCall?.[0])).toBe("ok🙂");

    const acceptedCallCount = onChange.mock.calls.length;
    act(() => {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: "🙂🙂🙂" },
      });
    });
    expect(view.state.doc.toString()).toBe("ok🙂");
    expect(onChange).toHaveBeenCalledTimes(acceptedCallCount);
  });

  it("destroys its heavy view and remounts only from controlled content", async () => {
    const first = render(
      <EditorAdapter
        ariaLabel="Edit stale.txt"
        path="stale.txt"
        content="stale disposable view"
        disabled={false}
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );
    const firstEditor = await screen.findByRole("textbox", {
      name: "Edit stale.txt",
    });
    expect(first.container.querySelectorAll(".cm-editor")).toHaveLength(1);
    const destroy = vi.spyOn(viewForEditor(firstEditor), "destroy");

    first.unmount();
    expect(destroy).toHaveBeenCalledOnce();
    expect(first.container.querySelector(".cm-editor")).toBeNull();

    const second = render(
      <EditorAdapter
        ariaLabel="Edit restored.txt"
        path="restored.txt"
        content="durable restored content"
        disabled={false}
        maxContentLength={1024}
        onChange={vi.fn()}
      />,
    );
    await screen.findByRole("textbox", { name: "Edit restored.txt" });
    expect(second.container.querySelectorAll(".cm-editor")).toHaveLength(1);
    expect(second.container.querySelector(".cm-content")).toHaveTextContent(
      "durable restored content",
    );
  });
});
