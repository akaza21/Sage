import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, onTestFinished, vi } from "vitest";

import { useEffect, useRef, useState } from "react";

import { useWorkbenchStore } from "../app/store";
import type { ActiveEditorState } from "../app/editorSlice";
import {
  subscribeEditorFocusRequest,
  takeEditorFocusRequest,
} from "../features/files/editorFocusRequest";
import { DirtyTabDialog } from "./DirtyTabDialog";

const initialState = useWorkbenchStore.getState();

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

const fileTabs = [
  {
    id: "file-a",
    kind: "file" as const,
    title: "main.ts",
    position: 0,
    pinned: false,
    dirty: true,
    resource: { rootId: "root-a", relativePath: "src/main.ts" },
  },
  {
    id: "file-b",
    kind: "file" as const,
    title: "other.ts",
    position: 1,
    pinned: false,
    dirty: true,
    resource: { rootId: "root-a", relativePath: "src/other.ts" },
  },
];

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

describe("DirtyTabDialog", () => {
  it("exposes a named modal and routes Escape through close cancellation", () => {
    useWorkbenchStore.setState({
      pendingCloseTabId: "file-a",
      tabs: fileTabs,
    });
    render(<DirtyTabDialog />);

    const dialog = screen.getByRole("dialog", {
      name: "Save changes to main.ts?",
    });
    expect(dialog).toHaveClass("dirty-dialog--editor");
    expect(dialog.parentElement).toBe(document.body);

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(useWorkbenchStore.getState().pendingCloseTabId).toBeNull();
  });

  it("returns the focus to where the close began when it is cancelled", () => {
    const editor = document.createElement("textarea");
    document.body.append(editor);
    onTestFinished(() => editor.remove());
    editor.focus();
    useWorkbenchStore.setState({ pendingCloseTabId: "file-a", tabs: fileTabs });
    render(<DirtyTabDialog />);
    const dialog = screen.getByRole("dialog", {
      name: "Save changes to main.ts?",
    });
    within(dialog).getByRole("button", { name: "Cancel" }).focus();

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(editor).toHaveFocus();
  });

  describe("Cancel after a close that began on a tab button", () => {
    function Editor() {
      const ref = useRef<HTMLTextAreaElement>(null);
      const [requests, setRequests] = useState(0);
      useEffect(
        () => subscribeEditorFocusRequest(() => setRequests((n) => n + 1)),
        [],
      );
      useEffect(() => {
        if (takeEditorFocusRequest(fileTabs[0].resource)) ref.current?.focus();
      }, [requests]);
      return <textarea aria-label="Editor" ref={ref} />;
    }

    function cancelFromCloseButton(activeEditor: ActiveEditorState | null) {
      const closeButton = document.createElement("button");
      document.body.append(closeButton);
      onTestFinished(() => closeButton.remove());
      closeButton.focus();
      useWorkbenchStore.setState({
        activeTabId: "file-a",
        activeEditor,
        pendingCloseTabId: "file-b",
        tabs: fileTabs,
      });
      // One root, so the dialog's focus restore and the editor's focus
      // request settle in the same commit, as in the workbench.
      render(
        <>
          <Editor />
          <DirtyTabDialog />
        </>,
      );
      const dialog = screen.getByRole("dialog", {
        name: "Save changes to other.ts?",
      });
      fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      return closeButton;
    }

    it("returns to the open editor of the active File", () => {
      cancelFromCloseButton({
        phase: "ready",
        resource: fileTabs[0].resource,
      } as ActiveEditorState);

      expect(screen.getByRole("textbox", { name: "Editor" })).toHaveFocus();
    });

    it("returns to the close button while the File is still loading", () => {
      const closeButton = cancelFromCloseButton({
        phase: "loading",
        resource: fileTabs[0].resource,
      });

      expect(closeButton).toHaveFocus();
    });
  });

  it("owns a pending Save by tab and refuses every dismissal until it settles", async () => {
    const save = deferred();
    const saveAndCloseTab = vi.fn(() => save.promise);
    const cancelCloseTab = vi.fn();
    useWorkbenchStore.setState({
      pendingCloseTabId: "file-a",
      tabs: fileTabs,
      cancelCloseTab,
      saveAndCloseTab,
    });
    render(<DirtyTabDialog />);

    const dialog = screen.getByRole("dialog", {
      name: "Save changes to main.ts?",
    });
    const saveButton = within(dialog).getByRole("button", { name: "Save" });
    fireEvent.click(saveButton);
    fireEvent.click(saveButton);

    expect(saveAndCloseTab).toHaveBeenCalledTimes(1);
    for (const action of within(dialog).getAllByRole("button")) {
      expect(action).toBeDisabled();
    }
    fireEvent.keyDown(dialog, { key: "Escape" });
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(cancelCloseTab).not.toHaveBeenCalled();
    expect(saveAndCloseTab).toHaveBeenCalledTimes(1);
    expect(dialog).toBeInTheDocument();

    await act(async () => save.resolve());
    expect(dialog).toBeInTheDocument();
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Save" }),
      ).toBeEnabled(),
    );
  });

  it("gives a different tab fresh actions while an old Discard settles", async () => {
    const discard = deferred();
    const discardAndCloseTab = vi.fn(() => discard.promise);
    useWorkbenchStore.setState({
      pendingCloseTabId: "file-a",
      tabs: fileTabs,
      discardAndCloseTab,
    });
    render(<DirtyTabDialog />);

    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    expect(discardAndCloseTab).toHaveBeenCalledTimes(1);

    act(() => useWorkbenchStore.setState({ pendingCloseTabId: "file-b" }));
    const nextDialog = screen.getByRole("dialog", {
      name: "Save changes to other.ts?",
    });
    expect(
      within(nextDialog).getByRole("button", { name: "Save" }),
    ).toBeEnabled();
    expect(
      within(nextDialog).getByRole("button", { name: "Discard" }),
    ).toBeEnabled();

    await act(async () => discard.resolve());
    expect(nextDialog).toBeInTheDocument();
  });
});
