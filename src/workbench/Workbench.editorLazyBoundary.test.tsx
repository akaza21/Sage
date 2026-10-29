import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { EditorDocument, WorkbenchTab } from "../domain/workbench";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";
import { settleLazyModules } from "../test/settleLazyModules";

const runtimeProbe = vi.hoisted(() => ({
  moduleLoaded: vi.fn(),
  create: vi.fn(),
  destroy: vi.fn(),
}));

vi.mock("../features/files/CodeMirrorEditorRuntime", () => {
  runtimeProbe.moduleLoaded();
  return {
    createCodeMirrorEditorRuntime: (options: {
      parent: HTMLElement;
      ariaLabel: string;
    }) => {
      runtimeProbe.create(options);
      const editor = options.parent.ownerDocument.createElement("div");
      editor.setAttribute("role", "textbox");
      editor.setAttribute("aria-label", options.ariaLabel);
      editor.setAttribute("contenteditable", "true");
      options.parent.append(editor);
      return {
        destroy: () => {
          runtimeProbe.destroy();
          editor.remove();
        },
        focus: () => editor.focus(),
        selection: () => ({ start: 0, end: 0 }),
        setSelection: vi.fn(),
        update: vi.fn(),
      };
    },
  };
});
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
const resource = { rootId: "root-a", relativePath: "src/lazy.ts" };
const version = { fingerprint: "base", byteLen: 15, modifiedAt: null };
const document: EditorDocument = {
  resource,
  disk: version,
  base: version,
  buffer: {
    fingerprint: "base",
    byteLen: 15,
    revision: 0,
    content: "const lazy = 1;",
  },
  lineEnding: "lf",
  dirty: false,
  conflict: null,
};
const tabs: WorkbenchTab[] = [
  {
    id: "chat:first",
    kind: "chat",
    title: "First",
    position: 0,
    pinned: false,
    dirty: false,
  },
  {
    id: "file:lazy",
    kind: "file",
    title: "lazy.ts",
    position: 1,
    pinned: false,
    dirty: false,
    resource,
  },
];

beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  runtimeProbe.moduleLoaded.mockClear();
  runtimeProbe.create.mockClear();
  runtimeProbe.destroy.mockClear();
  useWorkbenchStore.setState(initialState, true);
});

it("loads editor runtime only for an active File and destroys it on return to Chat", async () => {
  useWorkbenchStore.setState({
    tabs,
    activeTabId: "chat:first",
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
  render(<Workbench />);

  await act(async () => Promise.resolve());
  expect(screen.getByRole("region", { name: "Editor welcome" })).toBeVisible();
  expect(runtimeProbe.moduleLoaded).not.toHaveBeenCalled();
  expect(runtimeProbe.create).not.toHaveBeenCalled();

  act(() => useWorkbenchStore.setState({ activeTabId: "file:lazy" }));
  await settleLazyModules();

  await waitFor(() => expect(runtimeProbe.moduleLoaded).toHaveBeenCalledOnce());
  expect(runtimeProbe.create).toHaveBeenCalledOnce();
  expect(
    await screen.findByRole("textbox", { name: "Edit src/lazy.ts" }),
  ).toBeVisible();

  act(() => useWorkbenchStore.setState({ activeTabId: "chat:first" }));
  expect(runtimeProbe.destroy).toHaveBeenCalledOnce();
  expect(
    screen.queryByRole("textbox", { name: "Edit src/lazy.ts" }),
  ).toBeNull();
});
