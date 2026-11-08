import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  onTestFinished,
  vi,
} from "vitest";

import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { useUiAppearanceStore } from "../app/uiAppearanceStore";
import * as nativeBrowser from "../ports/nativeBrowserPort";
import { Workbench } from "./Workbench";
import { writeWorkbenchLayout } from "./workbenchLayoutStorage";
import { settleLazyModules } from "../test/settleLazyModules";
import { useSideChatState } from "./sideChatState";
import { useAgentChangesState } from "./agentChangesState";

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () =>
    createElement(
      "section",
      { "aria-label": "Agent chat" },
      "Active chat",
      createElement("textarea", { "aria-label": "Composer" }),
    ),
}));

async function selectWindowMode(mode: "Agent" | "Editor") {
  fireEvent.click(
    within(screen.getByRole("radiogroup", { name: "Window mode" })).getByRole(
      "radio",
      { name: mode },
    ),
  );
  await settleLazyModules();
}

function seedResourceChat() {
  useWorkbenchStore.setState({
    tabs: [
      {
        id: "chat:a",
        kind: "chat",
        title: "A",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
    activeTabId: "chat:a",
  });
}

async function openChatActions(title: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `Chat actions for ${title}` }),
  );
  await screen.findByRole("menu", { name: `Actions for ${title}` });
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
  // The Changes tab outlives a render, as it does across app remounts.
  useAgentChangesState.getState().close();
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

beforeEach(() => writeWorkbenchLayout({ windowMode: "editor" }));

afterEach(() => {
  cleanup();
  useSideChatState.setState({ sides: [], pairedId: null });
  useAgentSettingsState.getState().close();
  localStorage.clear();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  useWorkbenchStore.setState(initialState, true);
  vi.restoreAllMocks();
});

describe("compact Workbench project tools", () => {
  it("reveals navigation in one click after manually collapsing it before narrowing with Files open", async () => {
    compactViewport = false;
    vi.stubGlobal("innerWidth", 1280);
    seedResourceChat();
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: "Hide projects and chats" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Show files and preview · Waiting" }),
    );
    act(() => {
      vi.stubGlobal("innerWidth", 800);
      window.dispatchEvent(new Event("resize"));
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Show projects and chats" }),
    );
    expect(
      screen.getByRole("button", { name: "Hide projects and chats" }),
    ).toBeVisible();
    expect(
      container.querySelector(".agent-artifact-pane__canvas"),
    ).not.toBeVisible();
  });

  it("keeps an empty Agent resource pane collapsed until the user opens it", async () => {
    compactViewport = false;
    vi.stubGlobal("innerWidth", 1280);
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
    await selectWindowMode("Agent");

    const openResources = screen.getByRole("button", {
      name: "Show files and preview · Waiting",
    });
    expect(openResources).toBeVisible();
    expect(
      screen.queryByRole("separator", { name: "Resize files and preview" }),
    ).toBeNull();

    fireEvent.click(openResources);

    expect(
      screen.getByRole("separator", { name: "Resize files and preview" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", {
        name: "Collapse files, changes, and preview",
      }),
    ).toBeVisible();
  });

  it("folds navigation and opens narrow Agent resources over the whole Chat column", async () => {
    vi.stubGlobal("innerWidth", 680);
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

    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    const trigger = screen.getByRole("button", {
      name: "Show files and preview · Waiting",
    });
    expect(trigger.closest(".agent-chat-center__header")).not.toBeNull();
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    const body = container.querySelector(".workbench__body") as HTMLElement;
    expect(body.style.gridTemplateColumns).toBe("320px minmax(0, 1fr)");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    );

    const resources = screen.getByRole("dialog", { name: "Agent artifacts" });
    expect(resources).not.toHaveAttribute("aria-modal");
    expect(container.querySelector(".project-sidebar")).not.toHaveAttribute(
      "inert",
    );
    expect(body.style.gridTemplateColumns).toBe("46px minmax(0, 1fr)");
    expect(body.style.getPropertyValue("--agent-artifact-overlay-width")).toBe(
      "634px",
    );
    expect(container.querySelector(".center-region")).toHaveAttribute("inert");

    fireEvent.keyDown(resources, { key: "Escape" });
    expect(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    ).toBeVisible();
    expect(container.querySelector(".center-region")).not.toHaveAttribute(
      "inert",
    );
    expect(body.style.gridTemplateColumns).toBe("320px minmax(0, 1fr)");
  });

  it("folds navigation to its icon rail so Files sit beside Chat at 800px", async () => {
    compactViewport = false;
    vi.stubGlobal("innerWidth", 800);
    seedResourceChat();
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    const body = container.querySelector(".workbench__body") as HTMLElement;
    expect(body.style.gridTemplateColumns).toBe("326px 8px minmax(0, 1fr)");
    fireEvent.click(
      screen.getByRole("button", { name: "Show files and preview · Waiting" }),
    );
    expect(body.style.gridTemplateColumns).toBe(
      "46px minmax(0, 1fr) 8px 360px",
    );
    expect(container.querySelector(".center-region")).not.toHaveAttribute(
      "inert",
    );
    expect(screen.getByLabelText("Agent artifacts")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", {
        name: "Collapse files, changes, and preview",
      }),
    );
    expect(body.style.gridTemplateColumns).toBe("326px 8px minmax(0, 1fr)");
  });

  it("makes resources fully modal below the chat-column floor and dismisses outside", async () => {
    vi.stubGlobal("innerWidth", 360);
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat:a",
          kind: "chat",
          title: "A",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat:a",
    });
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    const trigger = screen.getByRole("button", {
      name: /Show files and preview/,
    });
    fireEvent.click(trigger);
    const pane = screen.getByRole("dialog", { name: "Agent artifacts" });
    expect(pane).toHaveAttribute("aria-modal", "true");
    expect(container.querySelector(".title-bar")).toHaveAttribute("inert");
    expect(container.querySelector(".status-bar")).toHaveAttribute("inert");
    expect(container.querySelector(".project-sidebar")).toHaveAttribute(
      "inert",
    );
    expect(container.querySelector(".center-region")).toHaveAttribute("inert");
    fireEvent.click(
      screen.getByRole("button", { name: "Dismiss files and preview" }),
    );
    expect(
      screen.queryByRole("dialog", { name: "Agent artifacts" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: /Show files and preview/ }),
    ).toHaveFocus();
    expect(container.querySelector(".project-sidebar")).not.toHaveAttribute(
      "inert",
    );
  });

  it("keeps the composer usable when resizing inline resources into an overlay", async () => {
    seedResourceChat();
    vi.stubGlobal("innerWidth", 1280);
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: /Show files and preview/ }),
    );
    const composer = screen.getByRole("textbox", { name: "Composer" });
    composer.focus();
    vi.stubGlobal("innerWidth", 680);
    fireEvent(window, new Event("resize"));
    expect(composer).toHaveFocus();
    expect(container.querySelector(".center-region")).not.toHaveAttribute(
      "inert",
    );
    expect(screen.getByLabelText("Agent artifacts")).not.toBeVisible();
  });

  it("keeps navigation focused when narrowing an open overlay dismisses it", async () => {
    seedResourceChat();
    vi.stubGlobal("innerWidth", 680);
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: /Show files and preview/ }),
    );
    const rail = () =>
      screen.getByRole("complementary", { name: "Projects and chats" });
    within(rail()).getByRole("button", { name: "Home" }).focus();
    // Below the modal threshold the overlay is dismissed, which unfolds the
    // rail; focus follows Home into the full rail instead of dropping.
    vi.stubGlobal("innerWidth", 360);
    fireEvent(window, new Event("resize"));
    expect(screen.getByLabelText("Agent artifacts")).not.toBeVisible();
    expect(within(rail()).getByRole("button", { name: "Home" })).toHaveFocus();
    expect(rail()).not.toHaveAttribute("inert");
    expect(container.querySelector(".center-region")).not.toHaveAttribute(
      "inert",
    );
  });

  it("keeps the folded navigation focused when a resource overlay widens into a docked pane", async () => {
    seedResourceChat();
    vi.stubGlobal("innerWidth", 680);
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: /Show files and preview/ }),
    );
    const navigation = screen.getByRole("complementary", {
      name: "Projects and chats",
    });
    const home = within(navigation).getByRole("button", { name: "Home" });
    home.focus();
    vi.stubGlobal("innerWidth", 900);
    fireEvent(window, new Event("resize"));
    expect(home).toHaveFocus();
    expect(navigation).not.toHaveAttribute("inert");
    expect(container.querySelector(".center-region")).not.toHaveAttribute(
      "inert",
    );
    expect(screen.getByLabelText("Agent artifacts")).toBeVisible();
  });

  it("does not leave a focusless modal open when navigation narrows", async () => {
    seedResourceChat();
    vi.stubGlobal("innerWidth", 680);
    render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: /Show files and preview/ }),
    );
    (document.activeElement as HTMLElement).blur();
    expect(document.body).toHaveFocus();
    vi.stubGlobal("innerWidth", 360);
    fireEvent(window, new Event("resize"));
    expect(screen.getByLabelText("Agent artifacts")).not.toBeVisible();
    expect(document.body).toHaveFocus();
  });

  it("renders only the focused Chat when compact instead of squeezing a pair", async () => {
    vi.stubGlobal("innerWidth", 680);
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
        {
          id: "chat:b",
          kind: "chat",
          title: "Chat: B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat:b",
    });

    render(<Workbench />);
    await selectWindowMode("Agent");

    expect(screen.getAllByRole("region", { name: "Agent chat" })).toHaveLength(
      1,
    );
    expect(screen.getByLabelText("Chat pane B")).toBeInTheDocument();
    expect(
      screen.queryByRole("separator", { name: "Resize chat panes" }),
    ).toBeNull();
  });

  it("lets forward Tab enter an opened resource overlay instead of dismissing it", async () => {
    seedResourceChat();
    vi.stubGlobal("innerWidth", 680);
    render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: "Show files and preview · Waiting" }),
    );
    const pane = screen.getByLabelText("Agent artifacts");
    expect(pane).toHaveFocus();
    fireEvent.keyDown(pane, { key: "Tab" });
    expect(
      screen.getByRole("button", {
        name: "Collapse files, changes, and preview",
      }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    ).toBeNull();
  });

  it("traps reverse overlay Tab until Escape returns focus and sleeps the canvas", async () => {
    seedResourceChat();
    vi.stubGlobal("innerWidth", 680);
    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", { name: "Show files and preview · Waiting" }),
    );
    const pane = screen.getByLabelText("Agent artifacts");
    expect(pane).toHaveFocus();
    expect(
      container.querySelector(".agent-artifact-pane__canvas"),
    ).not.toBeNull();
    fireEvent.keyDown(pane, { key: "Tab", shiftKey: true });
    expect(
      screen.getByRole("button", {
        name: "Collapse files, changes, and preview",
      }),
    ).toHaveFocus();
    fireEvent.keyDown(pane, { key: "Escape" });
    expect(
      screen.getByRole("button", { name: "Show files and preview · Waiting" }),
    ).toHaveFocus();
    expect(
      container.querySelector(".agent-artifact-pane__canvas"),
    ).not.toBeVisible();
  });

  it("keeps narrow Agent navigation visible and Chat interactive", async () => {
    vi.stubGlobal("innerWidth", 720);
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

    const { container } = render(<Workbench />);
    await selectWindowMode("Agent");

    expect(
      screen.getByRole("complementary", { name: "Projects and chats" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Open chat A, pinned" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("dialog", { name: "Projects and chats" }),
    ).toBeNull();

    expect(
      screen.queryByRole("button", { name: "Open projects and chats" }),
    ).toBeNull();
    expect(container.querySelector(".center-region")).not.toHaveAttribute(
      "inert",
    );
  });

  it("routes Chat navigation from the sidebar through authoritative activation", async () => {
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
    await selectWindowMode("Agent");

    fireEvent.click(
      screen.getByRole("button", { name: /^Open chat B(?:,|$)/ }),
    );

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
    await selectWindowMode("Agent");

    await openChatActions("B");
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename chat" }));
    const input = await screen.findByRole("textbox", {
      name: "New name for B",
    });
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
        screen.getByRole("button", { name: "Chat actions for B" }),
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
    await selectWindowMode("Agent");
    const sidebarScroll = container.querySelector(".project-sidebar__scroll");
    expect(sidebarScroll).not.toBeNull();
    sidebarScroll!.scrollTop = 72;

    await openChatActions("B");
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename chat" }));
    const input = await screen.findByRole("textbox", {
      name: "New name for B",
    });
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
        screen.getByRole("button", {
          name: "Chat actions for Release notes",
        }),
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
    await selectWindowMode("Agent");

    await openChatActions("B");
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename chat" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Cancel chat rename" }),
    );
    let trigger: HTMLElement;
    await vi.waitFor(() => {
      trigger = screen.getByRole("button", { name: "Chat actions for B" });
      expect(trigger).toHaveFocus();
    });

    await openChatActions("B");
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename chat" }));
    fireEvent.keyDown(
      await screen.findByRole("textbox", { name: "New name for B" }),
      {
        key: "Escape",
      },
    );
    await vi.waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Chat actions for B" }),
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
    await selectWindowMode("Agent");

    await openChatActions("B");
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive chat" }));
    expect(archiveChat).toHaveBeenCalledWith("chat:b", "project-a");
    const disclosure = screen.getByRole("button", {
      name: "Archived chats: 1",
    });
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(disclosure);
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    const region = screen.getByRole("region", { name: "Archived chats" });
    fireEvent.click(
      within(region).getByRole("button", { name: "Restore chat Old" }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat:old", "project-a", "row");

    act(() => {
      useWorkbenchStore.setState({
        tabs: [{ ...useWorkbenchStore.getState().tabs[0], pinned: false }],
      });
    });
    await openChatActions("A");
    expect(
      screen.getByRole("menuitem", { name: "Archive chat" }),
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
    await selectWindowMode("Agent");

    await openChatActions("B");
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename chat" }));
    const input = await screen.findByRole("textbox", {
      name: "New name for B",
    });
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

  it("routes the Agent New chat control through one creation action", async () => {
    const createChat = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      createChat,
      project: { id: "project-a", name: "A", roots: [] },
    });
    render(<Workbench />);
    await selectWindowMode("Agent");

    const newChatControls = screen.getAllByRole("button", {
      name: "New chat",
    });
    expect(newChatControls).toHaveLength(1);
    const beforeClick = createChat.mock.calls.length;
    fireEvent.click(newChatControls[0]);
    expect(createChat).toHaveBeenCalledTimes(beforeClick + 1);
  });

  it("creates a task from the new task keyboard shortcut", async () => {
    const createChat = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      createChat,
      project: { id: "project-a", name: "A", roots: [] },
    });
    render(<Workbench />);
    await selectWindowMode("Agent");

    const before = createChat.mock.calls.length;
    fireEvent.keyDown(window, { key: "n", ctrlKey: true });
    expect(createChat).toHaveBeenCalledTimes(before + 1);
  });

  it("reveals Settings when the rail gear is used from Agent mode", async () => {
    const openSettings = vi.fn(async () => undefined);
    useWorkbenchStore.setState({
      openSettings,
      project: { id: "project-a", name: "A", roots: [] },
    });
    render(<Workbench />);
    await selectWindowMode("Agent");

    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));

    expect(openSettings).not.toHaveBeenCalled();
    expect(screen.getByRole("radio", { name: "Agent" })).toBeChecked();
    expect(
      await screen.findByRole("dialog", { name: "Agent settings" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(screen.queryByRole("dialog", { name: "Agent settings" })).toBeNull();
  });

  it("guides an empty Editor workbench only through implemented entry points", async () => {
    render(<Workbench />);
    await settleLazyModules();

    const welcome = screen.getByRole("region", { name: "Editor welcome" });
    expect(
      within(welcome).getByRole("button", { name: "Browse files" }),
    ).toBeEnabled();
    expect(
      within(welcome).getByRole("button", { name: "Search files" }),
    ).toBeEnabled();
    expect(
      within(welcome).getByRole("button", { name: "Open Agent" }),
    ).toBeEnabled();
    expect(screen.queryByRole("button", { name: "New chat" })).toBeNull();
    expect(screen.queryByText("Username")).toBeNull();
  });

  it("keeps the settings shortcut in its window and preserves the focused chat", async () => {
    const openSettings = vi.fn(async () => undefined);
    seedResourceChat();
    useWorkbenchStore.setState({ openSettings });
    render(<Workbench />);
    await selectWindowMode("Agent");
    const activeChat = useWorkbenchStore.getState().activeTabId;
    const tabs = useWorkbenchStore.getState().tabs;
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    expect(
      await screen.findByRole("dialog", { name: "Agent settings" }),
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "Agent" })).toBeChecked();
    expect(useWorkbenchStore.getState().activeTabId).toBe(activeChat);
    expect(useWorkbenchStore.getState().tabs).toEqual(tabs);
    expect(openSettings).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByRole("button", { name: "Close settings" }), {
      key: "Escape",
    });
    expect(screen.queryByRole("dialog", { name: "Agent settings" })).toBeNull();
    await selectWindowMode("Editor");
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    expect(openSettings).toHaveBeenCalledOnce();
  });

  it("projects persisted interface appearance without changing editor settings", async () => {
    render(<Workbench />);
    await settleLazyModules();
    const initialAppearance = useUiAppearanceStore.getState().appearance;
    act(() =>
      useUiAppearanceStore.setState({
        appearance: { fontSize: 16, density: "compact" },
      }),
    );
    await vi.waitFor(() =>
      expect(
        document.documentElement.style.getPropertyValue("--font-size-md"),
      ).toBe("16px"),
    );
    expect(
      document.documentElement.style.getPropertyValue("--row-height-default"),
    ).toBe("28px");
    act(() => useUiAppearanceStore.setState({ appearance: initialAppearance }));
    await vi.waitFor(() =>
      expect(
        document.documentElement.style.getPropertyValue("--font-size-md"),
      ).toBe(`${initialAppearance.fontSize}px`),
    );
  });

  it.each([null, "file", "browser"] as const)(
    "restores the same chat pair and focus after settings with %s active",
    async (resourceKind) => {
      compactViewport = false;
      // Wide enough for the full rail beside an open browser resource.
      vi.stubGlobal("innerWidth", 1600);
      vi.spyOn(nativeBrowser, "closeBrowser").mockResolvedValue(undefined);
      vi.stubGlobal(
        "ResizeObserver",
        class {
          observe() {}
          unobserve() {}
          disconnect() {}
        },
      );
      useWorkbenchStore.setState({
        project: { id: "project-a", name: "A", roots: [] },
        tabs: ["a", "b", "c", "d"].map((id, position) => ({
          id: id === "d" ? "side-d" : `chat:${id}`,
          kind: "chat",
          title: `Chat ${id}`,
          position,
          pinned: false,
          dirty: false,
        })),
        activeTabId: "chat:a",
        activateTab: vi.fn(async (activeTabId: string) => {
          useWorkbenchStore.setState({ activeTabId });
        }),
      });
      render(<Workbench />);
      await selectWindowMode("Agent");
      act(() => useWorkbenchStore.setState({ activeTabId: "chat:c" }));
      act(() => {
        useSideChatState.setState({
          sides: [
            {
              id: "side-d",
              projectId: "project-a",
              parentChatId: "chat:c",
            },
          ],
          pairedId: "side-d",
        });
        useWorkbenchStore.setState({ activeTabId: "side-d" });
      });
      const pair = () =>
        Array.from(
          document.querySelectorAll<HTMLElement>(".agent-chat-pane"),
        ).map((pane) => pane.dataset.chatId);
      expect(pair()).toEqual(["chat:c", "side-d"]);
      const focusedPane = screen.getByRole("region", {
        name: "Chat pane Chat d",
      });
      if (resourceKind) {
        act(() =>
          useWorkbenchStore.setState((state) => ({
            tabs: [
              ...state.tabs,
              {
                id: "resource:settings-return",
                kind: resourceKind,
                title: "Settings return resource",
                position: state.tabs.length,
                pinned: false,
                dirty: false,
                resource:
                  resourceKind === "file"
                    ? { rootId: "root-a", relativePath: "return.ts" }
                    : undefined,
              },
            ],
            activeTabId: "resource:settings-return",
          })),
        );
      }
      expect(focusedPane).toHaveClass("agent-chat-pane--focused");
      fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
      await screen.findByRole("dialog", { name: "Agent settings" });
      expect(screen.queryByRole("region", { name: /^Chat pane / })).toBeNull();
      fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
      await settleLazyModules();
      expect(pair()).toEqual(["chat:c", "side-d"]);
      expect(
        document.querySelector('.agent-chat-pane[data-chat-id="side-d"]'),
      ).toBe(focusedPane);
      expect(focusedPane).toHaveClass("agent-chat-pane--focused");
    },
  );

  it("exposes only truthful focusable controls in the Project sidebar", async () => {
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
    await selectWindowMode("Agent");

    expect(
      screen.queryByRole("button", { name: "Open settings" }),
    ).toBeEnabled();

    const searchChats = within(
      screen.getByRole("complementary", { name: "Projects and chats" }),
    );
    const globalSearch = searchChats.getByRole("button", {
      name: "Search chats",
    });
    expect(globalSearch).toBeEnabled();
    globalSearch.focus();
    expect(globalSearch).toHaveFocus();
    fireEvent.click(globalSearch);
    await settleLazyModules();
    expect(
      await screen.findByRole("dialog", { name: "Search Chats" }),
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

  it("exposes bounded keyboard-resizable rails in each full workbench mode", async () => {
    seedResourceChat();
    compactViewport = false;
    vi.stubGlobal("innerWidth", 1280);
    render(<Workbench />);

    const editorRail = screen.getByRole("separator", {
      name: "Resize Editor tool rail",
    });
    expect(editorRail).toHaveAttribute("aria-valuemin", "200");
    expect(editorRail).toHaveAttribute("aria-valuemax", "420");
    expect(editorRail).toHaveAttribute("aria-valuenow", "272");
    fireEvent.keyDown(editorRail, { key: "ArrowRight" });
    expect(editorRail).toHaveAttribute("aria-valuenow", "280");

    await selectWindowMode("Agent");
    const agentRail = screen.getByRole("separator", {
      name: "Resize project and chat rail",
    });
    expect(agentRail).toHaveAttribute("aria-valuemin", "180");
    expect(agentRail).toHaveAttribute("aria-valuemax", "460");
    fireEvent.keyDown(agentRail, { key: "Home" });
    expect(agentRail).toHaveAttribute("aria-valuenow", "180");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    );
    expect(
      screen.getByRole("separator", { name: "Resize files and preview" }),
    ).toHaveAttribute("aria-valuenow", "576");
  });

  it("resizes untouched Agent proportions while retaining a manually sized rail", async () => {
    seedResourceChat();
    compactViewport = false;
    const originalWidth = window.innerWidth;
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1280,
    });
    render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    );

    const rail = screen.getByRole("separator", {
      name: "Resize project and chat rail",
    });
    const artifact = screen.getByRole("separator", {
      name: "Resize files and preview",
    });
    expect(rail).toHaveAttribute("aria-valuenow", "326");
    expect(artifact).toHaveAttribute("aria-valuenow", "576");

    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1380,
    });
    fireEvent(window, new Event("resize"));
    expect(rail).toHaveAttribute("aria-valuenow", "326");
    expect(artifact).toHaveAttribute("aria-valuenow", "621");

    fireEvent.keyDown(rail, { key: "ArrowRight" });
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1672,
    });
    fireEvent(window, new Event("resize"));
    expect(rail).toHaveAttribute("aria-valuenow", "334");
    expect(artifact).toHaveAttribute("aria-valuenow", "752");

    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: originalWidth,
    });
  });

  it("restores a dragged Agent artifact width after the window shrinks and grows", async () => {
    seedResourceChat();
    compactViewport = false;
    const originalWidth = window.innerWidth;
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1672,
    });
    render(<Workbench />);
    await selectWindowMode("Agent");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    );

    const artifact = screen.getByRole("separator", {
      name: "Resize files and preview",
    });
    expect(artifact).toHaveAttribute("aria-valuenow", "752");
    fireEvent.keyDown(artifact, { key: "End" });
    expect(artifact).toHaveAttribute("aria-valuenow", "1116");
    screen
      .getByRole("button", { name: "Collapse files, changes, and preview" })
      .focus();

    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 680,
    });
    fireEvent(window, new Event("resize"));
    expect(artifact).not.toBeInTheDocument();
    expect(screen.getByLabelText("Agent artifacts")).toHaveFocus();

    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 1672,
    });
    fireEvent(window, new Event("resize"));
    expect(
      screen.getByRole("separator", { name: "Resize files and preview" }),
    ).toHaveAttribute("aria-valuenow", "1116");

    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: originalWidth,
    });
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

    expect(splitter).toHaveAttribute("aria-valuenow", "298");
  });

  it("restores only persisted single-splitter widths on load", async () => {
    seedResourceChat();
    compactViewport = false;
    // Narrow enough that opening resources folds the rail to its icons.
    const originalWidth = window.innerWidth;
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 960,
    });
    onTestFinished(() => {
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: originalWidth,
      });
    });
    localStorage.setItem(
      "sage.workbench.layout.v1",
      JSON.stringify({
        editorToolRailWidth: 320,
        agentChatSplitPercent: 55,
        windowMode: "editor",
      }),
    );

    render(<Workbench />);

    expect(
      screen.getByRole("separator", { name: "Resize Editor tool rail" }),
    ).toHaveAttribute("aria-valuenow", "320");
    await selectWindowMode("Agent");
    expect(
      screen.getByRole("separator", { name: "Resize project and chat rail" }),
    ).toHaveAttribute("aria-valuenow", "326");
    fireEvent.click(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    );
    // Unpersisted, the Files open at their 45% share of the window.
    expect(
      screen.getByRole("separator", { name: "Resize files and preview" }),
    ).toHaveAttribute("aria-valuenow", "432");
    expect(screen.getByLabelText("Agent artifacts")).toBeVisible();
  });

  it("moves focus out of the center on a compact transition", async () => {
    await import("./SourceControlView");
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

    const drawerButton = await within(drawer).findByRole("button", {
      name: "Refresh Source Control",
    });
    drawerButton.focus();
    act(() => setCompactViewport(false));
    act(() => setCompactViewport(true));
    expect(drawerButton).toHaveFocus();
  });
});
