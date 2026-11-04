import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { createElement } from "react";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";

import { useSettingsPageRequest } from "../app/settingsPageRequest";
import { useWorkbenchStore } from "../app/store";
import type { EditorDocument } from "../domain/workbench";
import { contentText } from "../features/files/editorContent";
import { settleLazyModules } from "../test/settleLazyModules";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";
import {
  DEFAULT_WORKBENCH_WINDOW_MODE,
  EDITOR_CANVAS_KINDS,
  isWorkbenchWindowMode,
  WORKBENCH_WINDOW_MODES,
} from "./workbenchWindowMode";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement("section", { "aria-label": "Agent chat" }, "Active chat"),
}));

let compactViewport = false;

vi.mock("./useCompactWorkbenchLayout", () => ({
  useCompactWorkbenchLayout: () => compactViewport,
}));

const initialState = useWorkbenchStore.getState();

beforeAll(async () => {
  await import("./AgentWorkbenchBody");
});

beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  useAgentSettingsState.getState().close();
  localStorage.clear();
  compactViewport = false;
  useWorkbenchStore.setState(initialState, true);
  useSettingsPageRequest.setState({ page: null });
});

async function switchWindowMode(mode: "Agent" | "Editor") {
  fireEvent.click(
    within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
      "radio",
      { name: mode },
    ),
  );
  await settleLazyModules();
}

describe("D0 workbench window mode seam", () => {
  it("opens the shared Browser settings dialog from Editor and preserves its active resource", async () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "file:current",
          kind: "file",
          title: "current.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "current.ts" },
        },
      ],
      activeTabId: "file:current",
    });
    render(createElement(Workbench));
    await settleLazyModules();
    const activeTabId = useWorkbenchStore.getState().activeTabId;
    await act(() => useWorkbenchStore.getState().openSettings("browser"));
    await settleLazyModules();
    expect(
      screen.getByRole("dialog", { name: "Agent settings" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Browser" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(useWorkbenchStore.getState().activeTabId).toBe(activeTabId);
    expect(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Editor" },
      ),
    ).toBeChecked();
    act(() => useAgentSettingsState.getState().close());
    await switchWindowMode("Editor");
    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
    await settleLazyModules();
    fireEvent.click(screen.getByRole("tab", { name: "Browser" }));
    await settleLazyModules();
    expect(
      screen.getAllByRole("dialog", { name: "Agent settings" }),
    ).toHaveLength(1);
  });

  it("offers exactly Agent and Editor with Agent as the default", () => {
    expect([...WORKBENCH_WINDOW_MODES]).toEqual(["agent", "editor"]);
    expect(DEFAULT_WORKBENCH_WINDOW_MODE).toBe("agent");
    expect(isWorkbenchWindowMode("agent")).toBe(true);
    expect(isWorkbenchWindowMode("editor")).toBe(true);
    expect(isWorkbenchWindowMode("split")).toBe(false);
    expect(isWorkbenchWindowMode(1)).toBe(false);
  });

  it("admits only File, Diff, and Settings to the one Editor tab strip", () => {
    expect([...EDITOR_CANVAS_KINDS]).toEqual(["file", "diff", "settings"]);
  });

  it("shows Settings as a selected, closable Editor tab and returns to the exact file", async () => {
    useWorkbenchStore.setState({
      sidebarOpen: false,
      tabs: [
        {
          id: "file:current",
          kind: "file",
          title: "current.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/current.ts" },
        },
        {
          id: "file:other",
          kind: "file",
          title: "other.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
      ],
      activeTabId: "file:current",
      loadEditor: vi.fn(async () => undefined),
    });
    render(createElement(Workbench));
    await settleLazyModules();

    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
    await settleLazyModules();
    expect(
      await screen.findByRole("tab", { name: "Settings" }),
    ).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
    await settleLazyModules();
    expect(
      await screen.findByRole("region", { name: "Settings" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close Settings" }));
    await settleLazyModules();
    expect(screen.getByRole("tab", { name: "current.ts" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("region", { name: "Settings" })).toBeNull();
  });

  it("flushes exact Editor bytes before Settings and fails closed on snapshot error", async () => {
    const resource = { rootId: "root-a", relativePath: "src/current.ts" };
    const content = "const exactBytes = 'not snapshotted yet';\n";
    const version = {
      fingerprint: "base",
      byteLen: content.length,
      modifiedAt: null,
    };
    const document: EditorDocument = {
      resource,
      disk: version,
      base: version,
      buffer: {
        fingerprint: "working",
        byteLen: content.length,
        revision: 1,
        content,
      },
      lineEnding: "lf",
      dirty: true,
      conflict: null,
    };
    const readyEditor = {
      phase: "ready" as const,
      resource,
      document,
      content,
      revision: 1,
      persistedRevision: 0,
      persisting: false,
      saving: false,
      message: null,
    };
    const flushedContents: string[] = [];
    const flushActiveEditor = vi.fn(async () => {
      const editor = useWorkbenchStore.getState().activeEditor;
      const activeContent =
        editor?.phase === "ready" ? contentText(editor.content) : undefined;
      if (activeContent !== undefined) flushedContents.push(activeContent);
    });
    useWorkbenchStore.setState({
      phase: "ready",
      project: { id: "project-a", name: "A", roots: [] },
      tabs: [
        {
          id: "file:current",
          kind: "file",
          title: "current.ts",
          position: 0,
          pinned: false,
          dirty: true,
          resource,
        },
      ],
      activeTabId: "file:current",
      activeEditor: readyEditor,
      flushActiveEditor,
    });

    await useWorkbenchStore.getState().openSettings();
    expect(flushActiveEditor).toHaveBeenCalled();
    expect(flushedContents).toEqual([content]);
    expect(useWorkbenchStore.getState().activeTabId).toBe(
      "settings:appearance",
    );

    useWorkbenchStore.getState().closeTab("settings:appearance");
    useWorkbenchStore.setState({
      activeEditor: readyEditor,
      flushActiveEditor: vi.fn(async () => {
        throw new Error("Snapshot failed");
      }),
    });
    await useWorkbenchStore.getState().openSettings();

    await waitFor(() => {
      expect(useWorkbenchStore.getState().activeTabId).toBe("file:current");
    });
    expect(
      useWorkbenchStore.getState().tabs.some((tab) => tab.kind === "settings"),
    ).toBe(false);
    const editorAfterFailure = useWorkbenchStore.getState().activeEditor;
    expect(
      editorAfterFailure?.phase === "ready"
        ? editorAfterFailure.content
        : undefined,
    ).toBe(content);
    expect(useWorkbenchStore.getState().error).toBe("Snapshot failed");
  });

  it("keeps the Editor tool rail by default and swaps to Agent chrome on toggle", async () => {
    useWorkbenchStore.setState({ sidebarOpen: false });
    render(createElement(Workbench));
    await settleLazyModules();

    const modeToggle = screen.getByRole("radiogroup", { name: "Window mode" });
    const agentControl = within(modeToggle).getByRole("radio", {
      name: "Agent",
    });
    const editorControl = within(modeToggle).getByRole("radio", {
      name: "Editor",
    });
    expect(editorControl).toBeChecked();
    expect(agentControl).not.toBeChecked();
    expect(
      screen.getByRole("complementary", { name: "Project tools" }),
    ).toBeInTheDocument();

    fireEvent.click(agentControl);
    await settleLazyModules();

    const railToggle = screen.getByRole("radiogroup", { name: "Window mode" });
    expect(
      within(railToggle).getByRole("radio", { name: "Agent" }),
    ).toBeChecked();
    expect(
      within(railToggle).getByRole("radio", { name: "Editor" }),
    ).not.toBeChecked();
    expect(
      screen.queryByRole("complementary", { name: "Project tools" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Explorer" })).toBeNull();
    expect(
      await screen.findByRole("complementary", { name: "Projects and chats" }),
    ).toBeInTheDocument();

    fireEvent.click(within(railToggle).getByRole("radio", { name: "Editor" }));
    await settleLazyModules();

    expect(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Editor" },
      ),
    ).toBeChecked();
    expect(
      screen.getByRole("complementary", { name: "Project tools" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Explorer" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("complementary", { name: "Projects and chats" }),
    ).toBeNull();
    expect(
      screen.queryByRole("complementary", { name: "Agent artifacts" }),
    ).toBeNull();
  });

  it("rehydrates Agent mode from the existing bounded workbench layout", async () => {
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 272,
        agentChatSplitPercent: 50,
        windowMode: "agent",
      }),
    );

    render(createElement(Workbench));
    await settleLazyModules();

    expect(
      within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
        "radio",
        { name: "Agent" },
      ),
    ).toBeChecked();
  });

  it("keeps collapsed Agent resource access in the header and hides Editor chrome", async () => {
    useWorkbenchStore.setState({
      sidebarOpen: false,
      tabs: [
        {
          id: "file:a",
          kind: "file",
          title: "current.ts",
          position: 0,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/current.ts" },
        },
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "file:a",
      loadEditor: vi.fn(async () => undefined),
    });
    render(createElement(Workbench));
    await settleLazyModules();

    await switchWindowMode("Agent");

    expect(
      await screen.findByRole("complementary", { name: "Projects and chats" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Show files and preview · Waiting" }),
    ).toBeInTheDocument();
    expect(
      screen
        .getByRole("button", { name: "Show files and preview · Waiting" })
        .closest(".agent-chat-center__header"),
    ).not.toBeNull();
    expect(
      screen.queryByRole("complementary", { name: "Agent artifacts" }),
    ).toBeNull();
    expect(
      screen.queryByRole("navigation", { name: "Agent artifact tabs" }),
    ).toBeNull();
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Explorer" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Search" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Source Control" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Problems" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Outline" })).toBeNull();
  });

  it("places Editor project tools on the left of the file canvas", async () => {
    const { container } = render(createElement(Workbench));
    await settleLazyModules();
    const body = container.querySelector(".workbench__body");
    const tools = screen.getByRole("complementary", { name: "Project tools" });
    expect(body?.firstElementChild).toBe(tools);
    expect(
      screen.queryByRole("complementary", { name: "Projects and chats" }),
    ).toBeNull();
  });

  it("keeps one Editor resource tab strip across both window modes", async () => {
    render(createElement(Workbench));
    await settleLazyModules();

    expect(
      screen.getAllByRole("navigation", { name: "Open tabs" }),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("navigation", { name: "Agent artifact tabs" }),
    ).toBeNull();

    await switchWindowMode("Agent");

    expect(screen.queryByRole("navigation", { name: "Open tabs" })).toBeNull();
    expect(
      screen.queryByRole("navigation", { name: "Agent artifact tabs" }),
    ).toBeNull();
  });
});
