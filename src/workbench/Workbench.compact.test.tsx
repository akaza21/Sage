import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { Workbench } from "./Workbench";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement("section", { "aria-label": "Agent chat" }, "Active chat"),
}));

function selectWindowMode(mode: "Agent" | "Editor") {
  fireEvent.click(
    within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
      "radio",
      { name: mode },
    ),
  );
}

const initialState = useWorkbenchStore.getState();
let compactViewport = true;
let setCompactViewport: (compact: boolean) => void;

beforeEach(() => {
  Object.defineProperty(Element.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
  localStorage.clear();
  compactViewport = true;
  const listeners = new Set<() => void>();
  setCompactViewport = (compact) => {
    compactViewport = compact;
    listeners.forEach((listener) => listener());
  };
  vi.stubGlobal(
    "matchMedia",
    vi.fn().mockImplementation((query: string) => ({
      get matches() {
        return compactViewport && query === "(max-width: 760px)";
      },
      media: query,
      onchange: null,
      addEventListener: (_type: string, listener: () => void) =>
        listeners.add(listener),
      removeEventListener: (_type: string, listener: () => void) =>
        listeners.delete(listener),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  );
  useWorkbenchStore.setState({
    projects: [],
    project: null,
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    sidebarOpen: false,
    sidebarView: "explorer",
    repositories: { roots: [], truncated: false, elapsedMs: 0 },
  });
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  useWorkbenchStore.setState(initialState, true);
});

describe("compact Workbench project tools", () => {
  it("routes Chat navigation from the sidebar through authoritative activation", () => {
    const activateTab = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      activateTab,
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: true,
          dirty: false,
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
      activeTabId: "chat:a",
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    fireEvent.click(screen.getByTitle("Open chat B"));

    expect(activateTab).toHaveBeenCalledWith("chat:b");
  });

  it("renames a Chat from the compact accessible sidebar action", async () => {
    const activateTab = vi.fn(async () => undefined);
    const renameChat = vi.fn(async () => true);
    useWorkbenchStore.setState({
      activateTab,
      renameChat,
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: "chat:b",
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    fireEvent.click(screen.getByRole("button", { name: "Rename chat B" }));
    const input = screen.getByRole("textbox", { name: "New name for B" });
    fireEvent.change(input, { target: { value: "Release notes" } });
    fireEvent.click(screen.getByRole("button", { name: "Save chat name" }));

    await vi.waitFor(() => {
      expect(renameChat).toHaveBeenCalledWith(
        "chat:b",
        "Release notes",
        "project-a",
      );
      expect(
        screen.queryByRole("textbox", { name: "New name for B" }),
      ).toBeNull();
      expect(
        screen.getByRole("button", { name: "Rename chat B" }),
      ).toHaveFocus();
    });
    expect(activateTab).not.toHaveBeenCalled();
  });

  it("keeps the chat sidebar scroll position while saving a rename", async () => {
    let resolveRename!: (committed: boolean) => void;
    const renameChat = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          resolveRename = resolve;
        }),
    );
    useWorkbenchStore.setState({
      renameChat,
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: "chat:b",
    });
    const { container } = render(<Workbench />);
    selectWindowMode("Agent");
    const sidebarScroll = container.querySelector(".project-sidebar__scroll");
    expect(sidebarScroll).not.toBeNull();
    sidebarScroll!.scrollTop = 72;

    fireEvent.click(screen.getByRole("button", { name: "Rename chat B" }));
    const input = screen.getByRole("textbox", { name: "New name for B" });
    fireEvent.change(input, {
      target: { value: "Release notes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save chat name" }));
    expect(input).toBeDisabled();

    await act(async () => {
      useWorkbenchStore.setState((state) => ({
        tabs: state.tabs.map((tab) =>
          tab.id === "chat:b" ? { ...tab, title: "Chat: Release notes" } : tab,
        ),
      }));
      resolveRename(true);
    });

    await vi.waitFor(() => {
      expect(renameChat).toHaveBeenCalledWith(
        "chat:b",
        "Release notes",
        "project-a",
      );
      expect(
        screen.queryByRole("textbox", { name: "New name for B" }),
      ).toBeNull();
      expect(
        screen.getByRole("button", { name: "Rename chat Release notes" }),
      ).toHaveFocus();
      expect(container.querySelector(".project-sidebar__scroll")).toBe(
        sidebarScroll,
      );
      expect(sidebarScroll).toHaveProperty("scrollTop", 72);
    });
  });

  it("restores the same Chat rename trigger after Cancel and Escape", async () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: "chat:b",
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    let trigger = screen.getByRole("button", { name: "Rename chat B" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Cancel chat rename" }));
    await vi.waitFor(() => {
      trigger = screen.getByRole("button", { name: "Rename chat B" });
      expect(trigger).toHaveFocus();
    });

    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("textbox", { name: "New name for B" }), {
      key: "Escape",
    });
    await vi.waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Rename chat B" }),
      ).toHaveFocus();
    });
  });

  it("exposes bounded archive and restore actions in the Chat navigation", async () => {
    const archiveChat = vi.fn(async () => true);
    const restoreChat = vi.fn(async () => true);
    useWorkbenchStore.setState({
      archiveChat,
      restoreChat,
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: true,
          dirty: false,
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
      archivedChats: [{ id: "chat:old", title: "Chat: Old" }],
      activeTabId: "chat:a",
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    fireEvent.click(screen.getByRole("button", { name: "Archive chat B" }));
    expect(archiveChat).toHaveBeenCalledWith("chat:b", "project-a");
    const disclosure = screen.getByRole("button", { name: "Archived chats" });
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(disclosure);
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    const region = screen.getByRole("region", { name: "Archived chats" });
    fireEvent.click(
      within(region).getByRole("button", { name: "Restore chat Old" }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat:old", "project-a");

    act(() => {
      useWorkbenchStore.setState({
        tabs: [{ ...useWorkbenchStore.getState().tabs[0], pinned: false }],
      });
    });
    expect(
      screen.getByRole("button", { name: "Archive chat A" }),
    ).toBeDisabled();
  });

  it("keeps invalid rename feedback visible and focused with a File active", async () => {
    let resolveRename!: (committed: boolean) => void;
    const renameChat = vi.fn(() => {
      return new Promise<boolean>((resolve) => {
        resolveRename = resolve;
      });
    });
    useWorkbenchStore.setState({
      renameChat,
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 0,
          pinned: true,
          dirty: false,
        },
        {
          id: "file:a",
          kind: "file",
          title: "current.ts",
          position: 1,
          pinned: false,
          dirty: true,
          resource: { rootId: "root-a", relativePath: "src/current.ts" },
        },
      ],
      activeTabId: "file:a",
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    fireEvent.click(screen.getByRole("button", { name: "Rename chat B" }));
    const input = screen.getByRole("textbox", { name: "New name for B" });
    expect(input).toHaveAttribute("maxlength", "128");
    fireEvent.change(input, { target: { value: " \t " } });
    fireEvent.click(screen.getByRole("button", { name: "Save chat name" }));
    expect(input).toBeDisabled();

    await act(async () => {
      useWorkbenchStore.setState({
        error: "Chat title must not be empty after trimming",
      });
      resolveRename(false);
    });

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Chat title must not be empty after trimming",
    );
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAttribute("aria-describedby", alert.id);
    expect(input).not.toBeDisabled();
    expect(input).toHaveFocus();
    expect(useWorkbenchStore.getState().activeTabId).toBe("file:a");
  });

  it("routes the Agent New Chat control through one creation action", () => {
    const createChat = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      createChat,
      project: { id: "project-a", name: "A", roots: [] },
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    const newChatControls = screen.getAllByRole("button", {
      name: "New chat",
    });
    expect(newChatControls).toHaveLength(1);
    const beforeClick = createChat.mock.calls.length;
    fireEvent.click(newChatControls[0]);
    expect(createChat).toHaveBeenCalledTimes(beforeClick + 1);
  });

  it("guides an empty Editor workbench only through implemented entry points", () => {
    render(<Workbench />);

    expect(screen.getByText("No open files")).toBeInTheDocument();
    expect(
      screen.getByText("Open a file from Explorer, Search, or Source Control."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New chat" })).toBeNull();
    expect(screen.queryByText("Username")).toBeNull();
  });

  it("exposes only truthful focusable controls in the Project sidebar", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: true,
          dirty: false,
        },
      ],
      activeTabId: "chat:a",
    });
    render(<Workbench />);
    selectWindowMode("Agent");

    expect(
      screen.queryByRole("button", { name: "Open settings" }),
    ).not.toBeInTheDocument();

    const searchChats = within(
      screen.getByRole("complementary", { name: "Projects and chats" }),
    ).getByRole("button", { name: "Search chats" });
    expect(searchChats).toBeEnabled();
    searchChats.focus();
    expect(searchChats).toHaveFocus();
    fireEvent.click(searchChats);
    expect(
      screen.getByRole("dialog", { name: "Search Chats" }),
    ).toBeInTheDocument();
  });

  it("uses one accessible drawer and restores its rail focus after Escape", () => {
    const { container } = render(<Workbench />);
    const center = container.querySelector(".center-region");
    const sourceControl = screen.getByRole("button", {
      name: "Source Control",
    });

    expect(sourceControl).toHaveAttribute(
      "aria-controls",
      "project-tools-panel",
    );
    expect(sourceControl).toHaveAttribute("aria-expanded", "false");
    expect(center).not.toHaveAttribute("inert");

    fireEvent.click(sourceControl);
    const drawer = screen.getByRole("dialog", {
      name: "Source Control project tools",
    });
    expect(drawer).toHaveAttribute("id", "project-tools-panel");
    expect(drawer).toHaveFocus();
    expect(sourceControl).toHaveAttribute("aria-expanded", "true");
    expect(center).toHaveAttribute("inert");
    expect(center).toHaveAttribute("aria-hidden", "true");

    fireEvent.keyDown(drawer, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(sourceControl).toHaveFocus();
    expect(sourceControl).toHaveAttribute("aria-expanded", "false");
    expect(center).not.toHaveAttribute("inert");
    expect(center).not.toHaveAttribute("aria-hidden");
  });

  it("exposes bounded keyboard-resizable rails in each full workbench mode", () => {
    compactViewport = false;
    render(<Workbench />);

    const editorRail = screen.getByRole("separator", {
      name: "Resize Editor tool rail",
    });
    expect(editorRail).toHaveAttribute("aria-valuemin", "200");
    expect(editorRail).toHaveAttribute("aria-valuemax", "420");
    expect(editorRail).toHaveAttribute("aria-valuenow", "272");
    fireEvent.keyDown(editorRail, { key: "ArrowRight" });
    expect(editorRail).toHaveAttribute("aria-valuenow", "280");

    selectWindowMode("Agent");
    const agentRail = screen.getByRole("separator", {
      name: "Resize project and chat rail",
    });
    expect(agentRail).toHaveAttribute("aria-valuemin", "180");
    expect(agentRail).toHaveAttribute("aria-valuemax", "380");
    fireEvent.keyDown(agentRail, { key: "Home" });
    expect(agentRail).toHaveAttribute("aria-valuenow", "180");
    expect(
      screen.getByRole("separator", { name: "Resize files and preview" }),
    ).toHaveAttribute("aria-valuenow", "360");
  });

  it("resizes the Editor sidebar from its visible sidebar edge", () => {
    compactViewport = false;
    const { container } = render(<Workbench />);
    const body = container.querySelector(".workbench__body") as HTMLElement;
    const splitter = screen.getByRole("separator", {
      name: "Resize Editor tool rail",
    });
    vi.spyOn(body, "getBoundingClientRect").mockReturnValue({
      bottom: 700,
      height: 650,
      left: 0,
      right: 1200,
      top: 50,
      width: 1200,
      x: 0,
      y: 50,
      toJSON: () => ({}),
    });

    fireEvent.pointerDown(splitter, { pointerId: 1, clientX: 346 });

    expect(splitter).toHaveAttribute("aria-valuenow", "302");
  });

  it("restores only persisted single-splitter widths on load", () => {
    compactViewport = false;
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 320,
        agentChatSplitPercent: 55,
      }),
    );

    render(<Workbench />);

    expect(
      screen.getByRole("separator", { name: "Resize Editor tool rail" }),
    ).toHaveAttribute("aria-valuenow", "320");
    selectWindowMode("Agent");
    expect(
      screen.getByRole("separator", { name: "Resize project and chat rail" }),
    ).toHaveAttribute("aria-valuenow", "248");
    expect(
      screen.getByRole("separator", { name: "Resize files and preview" }),
    ).toHaveAttribute("aria-valuenow", "360");
  });

  it("moves focus out of the center on a compact transition", () => {
    compactViewport = false;
    useWorkbenchStore.setState({
      sidebarOpen: true,
      sidebarView: "sourceControl",
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
      ],
      activeTabId: "file:a",
      loadEditor: vi.fn(async () => undefined),
    });
    const { container } = render(<Workbench />);
    const center = container.querySelector(".center-region");
    const centerButton = within(center as HTMLElement).getByRole("tab", {
      name: /current\.ts/,
    });
    centerButton.focus();

    act(() => setCompactViewport(true));
    const drawer = screen.getByRole("dialog", {
      name: "Source Control project tools",
    });
    expect(drawer).toHaveFocus();
    expect(center).toHaveAttribute("inert");

    const drawerButton = within(drawer).getByRole("button", {
      name: "Refresh Source Control",
    });
    drawerButton.focus();
    act(() => setCompactViewport(false));
    act(() => setCompactViewport(true));
    expect(drawerButton).toHaveFocus();
  });
});
