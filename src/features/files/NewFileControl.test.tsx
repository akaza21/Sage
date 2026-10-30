import {
  act,
  cleanup,
  waitFor,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { ActiveEditorState } from "./fileEditorController";
import * as focusRequest from "./editorFocusRequest";
import { NewFileControl } from "./NewFileControl";

const initialState = useWorkbenchStore.getState();
const parent = { rootId: "root-a", relativePath: "src" };
const created = { rootId: "root-a", relativePath: "src/new.ts" };
const existing = { rootId: "root-a", relativePath: "src/old.ts" };

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

const loading = (resource: typeof created) =>
  ({ phase: "loading", resource }) as ActiveEditorState;
const ready = (resource: typeof created) =>
  ({ phase: "ready", resource }) as unknown as ActiveEditorState;

async function createWith(after: ActiveEditorState | null) {
  useWorkbenchStore.setState({
    project: { id: "p", name: "Sage", roots: [] },
    activeEditor: ready(existing),
    createProjectFile: vi.fn(async () => {
      useWorkbenchStore.setState({ activeEditor: after });
      return true;
    }),
  });
  render(<NewFileControl parent={parent} rootName="Sage" />);
  const trigger = screen.getByRole("button", { name: "New File" });
  fireEvent.click(trigger);
  fireEvent.change(screen.getByRole("textbox", { name: "File name" }), {
    target: { value: "new.ts" },
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Create" }));
  });
  return trigger;
}

describe("New File focus", () => {
  it("hands focus to the editor of the File it opened", async () => {
    const trigger = await createWith(ready(created));
    // The request module loads lazily.
    await waitFor(() =>
      expect(focusRequest.takeEditorFocusRequest(created)).toBe(true),
    );
    expect(trigger).not.toHaveFocus();
  });

  it.each([
    ["was not confirmed", ready(existing)],
    ["did not load", loading(created)],
  ])("returns focus to the button when the File %s", async (_, after) => {
    const trigger = await createWith(after);
    await waitFor(() => expect(trigger).toHaveFocus());
    await act(() => new Promise((resolve) => setTimeout(resolve, 50)));
    expect(focusRequest.takeEditorFocusRequest(created)).toBe(false);
  });
});
