import { createRef } from "react";
import { Transaction } from "@codemirror/state";
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

import { EditorAdapter, type EditorAdapterHandle } from "./EditorAdapter";

afterEach(cleanup);

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
    expect(onChange).toHaveBeenLastCalledWith("two");

    fireEvent.keyDown(editor, { key: "z", ctrlKey: true });
    expect(view.state.doc.toString()).toBe("one");
    expect(onChange).toHaveBeenLastCalledWith("one");

    fireEvent.keyDown(editor, { key: "y", ctrlKey: true });
    expect(view.state.doc.toString()).toBe("two");
    expect(onChange).toHaveBeenLastCalledWith("two");
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
    expect(onChange).toHaveBeenLastCalledWith("ok🙂");

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
