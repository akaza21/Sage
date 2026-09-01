import { EditorView } from "@codemirror/view";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { EditorDocument, WorkbenchTab } from "../domain/workbench";
import { Workbench } from "./Workbench";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <section>Active chat</section>,
}));
vi.mock("../features/diff/DiffSurface", () => ({
  DiffSurface: () => <section>Diff</section>,
}));
vi.mock("./DirtyTabDialog", () => ({ DirtyTabDialog: () => null }));
vi.mock("./LeftSidebar", () => ({ LeftSidebar: () => null }));
vi.mock("./RightSidebar", () => ({ RightSidebar: () => null }));
vi.mock("./StatusBar", () => ({ StatusBar: () => null }));
vi.mock("./TabStrip", () => ({ TabStrip: () => null }));
vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => false,
}));

const initialState = useWorkbenchStore.getState();
const resource = { rootId: "root-a", relativePath: "src/current.ts" };
const version = { fingerprint: "base", byteLen: 18, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: {
    fingerprint: "base",
    byteLen: 18,
    revision: 0,
    content: "const current = 1;",
  },
  lineEnding: "lf",
  dirty: false,
  conflict: null,
};
const tabs: WorkbenchTab[] = [
  {
    id: "file:current",
    kind: "file",
    title: "current.ts",
    position: 0,
    pinned: false,
    dirty: false,
    resource,
  },
  {
    id: "chat:next",
    kind: "chat",
    title: "Next",
    position: 1,
    pinned: false,
    dirty: false,
  },
];

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

it("destroys the heavy editor when a non-File tab becomes active", async () => {
  useWorkbenchStore.setState({
    tabs,
    activeTabId: "file:current",
    loadEditor: vi.fn(async () => undefined),
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
  const { container } = render(<Workbench />);
  const editor = await screen.findByRole("textbox", {
    name: "Edit src/current.ts",
  });
  const view = EditorView.findFromDOM(editor);
  if (!view) {
    throw new Error("Expected a mounted CodeMirror editor");
  }
  const destroy = vi.spyOn(view, "destroy");

  act(() => useWorkbenchStore.setState({ activeTabId: "chat:next" }));

  expect(
    screen.getByRole("region", { name: "Editor welcome" }),
  ).toBeVisible();
  expect(container.querySelector(".cm-editor")).toBeNull();
  expect(destroy).toHaveBeenCalledOnce();
});
