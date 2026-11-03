import { EditorView } from "@codemirror/view";
import { act, cleanup, render, screen } from "@testing-library/react";
import { Profiler, type ComponentType, type ReactNode } from "react";
import { beforeEach, afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import type { EditorDocument, WorkbenchTab } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { settleLazyModules } from "../test/settleLazyModules";
import type * as DirtyTabDialogModule from "./DirtyTabDialog";
import type * as RightSidebarModule from "./RightSidebar";
import type * as StatusBarModule from "./StatusBar";
import type * as TabStripModule from "./TabStrip";
import type * as TitleBarModule from "./TitleBar";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";

// Each region outside the File surface commits under its own Profiler, so a
// re-render caused by its own store subscription is counted too.
const commits = vi.hoisted(() => new Map<string, number>());
const profiled = vi.hoisted(
  () =>
    <P extends object>(id: string, Component: ComponentType<P>) =>
    (props: P) => (
      <ProfiledRegion id={id}>
        <Component {...props} />
      </ProfiledRegion>
    ),
);
function ProfiledRegion({ id, children }: { id: string; children: ReactNode }) {
  return (
    <Profiler
      id={id}
      onRender={() => commits.set(id, (commits.get(id) ?? 0) + 1)}
    >
      {children}
    </Profiler>
  );
}

vi.mock("./TabStrip", async (original) => {
  const module = await original<typeof TabStripModule>();
  return { TabStrip: profiled("TabStrip", module.TabStrip) };
});
vi.mock("./TitleBar", async (original) => {
  const module = await original<typeof TitleBarModule>();
  return { TitleBar: profiled("TitleBar", module.TitleBar) };
});
vi.mock("./StatusBar", async (original) => {
  const module = await original<typeof StatusBarModule>();
  return { StatusBar: profiled("StatusBar", module.StatusBar) };
});
vi.mock("./RightSidebar", async (original) => {
  const module = await original<typeof RightSidebarModule>();
  return { RightSidebar: profiled("RightSidebar", module.RightSidebar) };
});
vi.mock("./DirtyTabDialog", async (original) => {
  const module = await original<typeof DirtyTabDialogModule>();
  return { DirtyTabDialog: profiled("DirtyTabDialog", module.DirtyTabDialog) };
});
vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <section>Active chat</section>,
}));
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
];

beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

it("re-renders nothing outside the File surface for keystrokes after the first", async () => {
  vi.spyOn(
    getWorkbenchPorts().editor,
    "snapshotEditorBuffer",
  ).mockImplementation(async (request) => ({
    revision: request.revision,
    persistedAt: 1,
    dirty: true,
  }));
  useWorkbenchStore.setState({
    tabs,
    activeTabId: "file:current",
    sidebarOpen: true,
    sidebarView: "explorer",
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
  await settleLazyModules();
  const editor = await screen.findByRole("textbox", {
    name: "Edit src/current.ts",
  });
  const view = EditorView.findFromDOM(editor);
  if (!view) throw new Error("Expected a mounted CodeMirror editor");
  const type = (text: string) =>
    act(() => {
      view.dispatch(view.state.replaceSelection(text));
    });

  // The first keystroke flips the tab to dirty, which the tab strip shows.
  type("a");
  expect(useWorkbenchStore.getState().tabs[0]?.dirty).toBe(true);
  for (const region of ["TabStrip", "TitleBar", "StatusBar", "RightSidebar"]) {
    expect(commits.get(region), `${region} mounted`).toBeGreaterThan(0);
  }
  expect(commits.get("DirtyTabDialog") ?? 0).toBe(0);
  commits.clear();

  for (const text of ["b", "c", "d", "e"]) type(text);

  expect(useWorkbenchStore.getState().activeEditor).toMatchObject({
    revision: 5,
  });
  // StatusBar shows Ln and Col, so it may commit for the caret; nothing else.
  const { StatusBar: statusCommits = 0, ...others } =
    Object.fromEntries(commits);
  expect(others).toEqual({});
  expect(statusCommits).toBeLessThanOrEqual(4);
});
