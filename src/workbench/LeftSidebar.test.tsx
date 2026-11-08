import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { resetChatReadMarks } from "../features/chat/acp/chatReadMarks";
import {
  clearChatSearchFolderRequest,
  hasChatSearchFolderRequest,
  requestChatSearchInFolder,
} from "../features/chatSearch/chatSearchScope";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { LeftSidebar } from "./LeftSidebar";
import { MAX_AGENT_RAIL_PROJECT_PREVIEW_CHATS } from "./agentChatRail";
import { resetAgentRailInteractions } from "./agentRailInteractions";
import {
  resetAgentRailViewPrefs,
  setAgentRailOrganize,
} from "./agentRailViewPrefs";

it("leaves the brand and the window mode switch to the title bar", () => {
  render(<LeftSidebar />);
  expect(screen.queryByRole("banner")).toBeNull();
  expect(screen.queryByRole("radiogroup", { name: "Window mode" })).toBeNull();
});

it("provides a separate icon rail and sidebar header with reachable chat actions", () => {
  const createChat = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    createChat,
  });
  render(<LeftSidebar />);
  expect(
    screen.getByRole("navigation", { name: "Primary navigation" }),
  ).toBeInTheDocument();
  expect(
    screen
      .getByRole("complementary", { name: "Projects and chats" })
      .querySelector(".project-sidebar__brand"),
  ).toHaveTextContent("Sage");
  fireEvent.click(
    within(
      screen.getByRole("navigation", { name: "Primary navigation" }),
    ).getByRole("button", { name: "Home" }),
  );
  expect(createChat).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "New chat" }));
  expect(createChat).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("searchbox", { name: "Search tasks" })).toBeNull();
  expect(screen.getByRole("button", { name: "Search chats" })).toBeEnabled();
  expect(
    screen.queryByRole("button", { name: "Open settings from rail" }),
  ).toBeNull();
});

it("dismisses the live palette without hiding persistent Agent navigation", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
  });
  render(<LeftSidebar />);

  fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
  const search = await screen.findByRole("searchbox", { name: "Search Chats" });
  fireEvent.change(search, { target: { value: "example" } });
  fireEvent.keyDown(search, { key: "Escape" });

  expect(screen.queryByRole("searchbox", { name: "Search tasks" })).toBeNull();
  expect(
    screen.getByRole("complementary", { name: "Projects and chats" }),
  ).toBeVisible();
  expect(
    screen.queryByRole("button", { name: "Open projects and chats" }),
  ).toBeNull();
  await vi.waitFor(() =>
    expect(screen.getByRole("button", { name: "Search chats" })).toHaveFocus(),
  );

  fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
  expect(screen.getByRole("searchbox", { name: "Search Chats" })).toHaveValue(
    "",
  );
});

it("keeps open and running chats keyboard-reachable in the sidebar", () => {
  const activateTab = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
    tabs: [
      {
        id: "chat:open",
        kind: "chat",
        title: "Open task",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
      {
        id: "chat:run",
        kind: "chat",
        title: "Running task",
        position: 1,
        pinned: false,
        dirty: false,
        status: "working",
      },
    ],
    activeTabId: "chat:open",
    activateTab,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  const running = screen.getByRole("button", {
    name: "Open chat Running task",
  });
  expect(running).toBeEnabled();
  running.focus();
  expect(running).toHaveFocus();
  fireEvent.click(running);
  expect(activateTab).toHaveBeenCalledWith("chat:run");
  expect(
    screen.getByRole("button", { name: "Open chat Open task" }),
  ).toBeEnabled();
});

it("opens Agent settings from the rail footer without opening an Editor tab", () => {
  const openSettings = vi.fn();
  useWorkbenchStore.setState({ openSettings });
  render(<LeftSidebar />);
  fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
  expect(openSettings).not.toHaveBeenCalled();
  expect(useAgentSettingsState.getState().page).toBe("Appearance");
});

const initialState = useWorkbenchStore.getState();

it.each([false, true])(
  "scrolls a newly selected Chat beyond eight rows into view (collapsed: %s) without jumping on status updates",
  async (collapsed) => {
    const prior = Element.prototype.scrollIntoView;
    const seen: string[] = [];
    Element.prototype.scrollIntoView = function () {
      seen.push((this as HTMLElement).dataset.chatId ?? "");
    };
    const chats = Array.from({ length: 12 }, (_, position) => ({
      id: `chat-${position}`,
      kind: "chat" as const,
      title: `Chat ${position}`,
      position,
      pinned: false,
      dirty: false,
      status: "idle" as const,
    }));
    useWorkbenchStore.setState({
      project: { id: "sage", name: "Sage", roots: [] },
      projects: [{ id: "sage", name: "Sage", rootCount: 0, lastOpenedAt: 1 }],
      tabs: chats,
      activeTabId: "chat-0",
      projectTransitioning: false,
      loadRepositoryInventory: vi.fn(async () => undefined),
    });
    try {
      render(<LeftSidebar />);
      expect(seen).toEqual(["chat-0"]);
      if (collapsed)
        fireEvent.click(screen.getByRole("button", { name: "Project Sage" }));
      act(() => useWorkbenchStore.setState({ activeTabId: "chat-11" }));
      await waitFor(() => expect(seen).toEqual(["chat-0", "chat-11"]));
      expect(
        within(screen.getByRole("region", { name: "Projects" })).getAllByRole(
          "button",
          { name: /^Open chat / },
        ),
      ).toHaveLength(MAX_AGENT_RAIL_PROJECT_PREVIEW_CHATS);
      act(() =>
        useWorkbenchStore.setState({
          tabs: chats.map((chat) => ({ ...chat, updatedAt: 10 })),
        }),
      );
      expect(seen).toEqual(["chat-0", "chat-11"]);
    } finally {
      if (prior) Element.prototype.scrollIntoView = prior;
      else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
    }
  },
);

afterEach(() => {
  cleanup();
  resetAgentRailInteractions();
  useAgentSettingsState.getState().close();
  clearChatSearchFolderRequest();
  resetChatReadMarks();
  resetAgentRailViewPrefs();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

it("announces the New chat shortcut without printing it in the row", () => {
  render(<LeftSidebar />);
  const button = screen.getByRole("button", { name: "New chat" });
  expect(button).toHaveTextContent(/^New chat$/);
  expect(button).toHaveAttribute("aria-keyshortcuts", "Control+N");
});

it("keeps New chat available when no folder project is open", () => {
  const createChat = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: null,
    projects: [],
    createChat,
  });
  render(<LeftSidebar />);
  const button = screen.getByRole("button", { name: "New chat" });
  expect(button).toBeEnabled();
  expect(
    screen.getByText(
      "New chat starts under Local. Create a project when you want files.",
    ),
  ).toBeInTheDocument();
  fireEvent.click(button);
  expect(createChat).toHaveBeenCalledTimes(1);
});

it("lists chats under every project folder and opens a foreign chat", () => {
  const switchProject = vi.fn(async () => undefined);
  const openRailChat = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Research agent memory",
            pinned: false,
            position: 0,
          },
        ],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:sage",
        kind: "chat",
        title: "Continue prior chat goal",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:sage",
    switchProject,
    openRailChat,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  expect(
    screen.getByRole("button", {
      name: /^Open chat Continue prior chat goal(?:,|$)/,
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", {
      name: /^Open chat Research agent memory(?:,|$)/,
    }),
  ).toBeInTheDocument();
  fireEvent.click(
    screen.getByRole("button", {
      name: /^Open chat Research agent memory(?:,|$)/,
    }),
  );
  expect(openRailChat).toHaveBeenCalledWith("cross", "chat:cross");
  expect(switchProject).not.toHaveBeenCalled();
});

it("pins and moves a foreign-folder chat without switching first", async () => {
  const pinChat = vi.fn(async () => undefined);
  const moveChat = vi.fn(async () => undefined);
  const switchProject = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Research agent memory",
            pinned: false,
            position: 0,
          },
        ],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
      { id: "work", name: "Work", rootCount: 1, lastOpenedAt: 3 },
    ],
    tabs: [
      {
        id: "chat:sage",
        kind: "chat",
        title: "Continue prior chat goal",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:sage",
    pinChat,
    moveChat,
    switchProject,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  fireEvent.click(
    screen.getByRole("button", {
      name: "Chat actions for Research agent memory",
    }),
  );
  fireEvent.click(await screen.findByRole("menuitem", { name: "Pin chat" }));
  expect(pinChat).toHaveBeenCalledWith("chat:cross", "cross");
  expect(switchProject).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Chat actions for Research agent memory",
    }),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Move to project" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Move chat to Work" }));
  expect(moveChat).toHaveBeenCalledWith("chat:cross", "work", "cross");
  expect(switchProject).not.toHaveBeenCalled();
});

it("restores a foreign-folder archived chat without switching first", () => {
  const restoreChat = vi.fn(async () => true);
  const switchProject = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [],
        railArchived: [{ id: "chat:old", title: "Research" }],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:sage",
        kind: "chat",
        title: "Continue prior chat goal",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:sage",
    archivedChats: [],
    restoreChat,
    switchProject,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  fireEvent.click(screen.getByRole("button", { name: /^Archived chats:/ }));
  fireEvent.click(
    screen.getByRole("button", { name: "Restore chat Research" }),
  );
  expect(restoreChat).toHaveBeenCalledWith("chat:old", "cross", "row");
  expect(switchProject).not.toHaveBeenCalled();
});

it("creates a chat from a foreign project menu without switching first", async () => {
  const createChat = vi.fn(async () => undefined);
  const switchProject = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Research agent memory",
            pinned: false,
            position: 0,
          },
        ],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:sage",
        kind: "chat",
        title: "Continue prior chat goal",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:sage",
    createChat,
    switchProject,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  fireEvent.click(
    await screen.findByRole("button", { name: "Project actions for Cross" }),
  );
  fireEvent.click(
    screen.getByRole("menuitem", { name: "New chat in project" }),
  );
  expect(createChat).toHaveBeenCalledWith("cross");
  expect(switchProject).not.toHaveBeenCalled();
});

it("opens Chat search on a truncated foreign folder without switching", async () => {
  const switchProject = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Research agent memory",
            pinned: false,
            position: 0,
          },
        ],
        railChatsTruncated: true,
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:sage",
        kind: "chat",
        title: "Continue prior chat goal",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:sage",
    switchProject,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  fireEvent.click(screen.getByRole("button", { name: "Show more" }));
  expect(switchProject).not.toHaveBeenCalled();
  expect(
    await screen.findByRole("button", { name: "Search in" }),
  ).toHaveTextContent("Cross");
  expect(useWorkbenchStore.getState().project?.id).toBe("sage");
});

it("continues In one list into Chat search when a folder holds more chats than the rail", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Research agent memory",
            pinned: false,
            position: 0,
            updatedAt: 5,
          },
        ],
        railChatsTruncated: true,
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [],
    activeTabId: null,
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  act(() => setAgentRailOrganize("list"));
  render(<LeftSidebar />);
  const recents = screen.getByRole("region", { name: "Recents" });
  fireEvent.click(
    await within(recents).findByRole("button", { name: "Show more" }),
  );
  expect(
    await screen.findByRole("button", { name: "Search in" }),
  ).toHaveTextContent("Cross");
});

it("mounts the lazy Chat search dialog only for a folder request, including one made before the rail", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  const { unmount } = render(<LeftSidebar />);
  await Promise.resolve();
  expect(screen.queryByRole("dialog", { name: "Search Chats" })).toBeNull();
  unmount();

  requestChatSearchInFolder("sage");
  render(<LeftSidebar />);
  const dialog = await screen.findByRole("dialog", { name: "Search Chats" });
  expect(within(dialog).getByRole("searchbox")).toHaveFocus();
  expect(hasChatSearchFolderRequest()).toBe(false);
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog", { name: "Search Chats" })).toBeNull();
  act(() => requestChatSearchInFolder("sage"));
  expect(screen.getByRole("dialog", { name: "Search Chats" })).toBeTruthy();
});

it("organizes the sidebar by project or in one list, in either sort order", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross",
            title: "Cross newest",
            pinned: false,
            position: 0,
            updatedAt: 300,
          },
        ],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:first",
        kind: "chat",
        title: "Sage first",
        position: 0,
        pinned: false,
        dirty: false,
        updatedAt: 100,
      },
      {
        id: "chat:second",
        kind: "chat",
        title: "Sage second",
        position: 1,
        pinned: false,
        dirty: false,
        updatedAt: 200,
      },
    ],
    activeTabId: "chat:first",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  const titles = (region: HTMLElement) =>
    within(region)
      .getAllByRole("button", { name: /^Open chat / })
      .map((button) => button.textContent);
  const choose = async (submenu: string, choice: string) => {
    fireEvent.click(screen.getByRole("button", { name: "Sidebar options" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: submenu }));
    fireEvent.click(await screen.findByRole("menuitemradio", { name: choice }));
  };

  const sage = () =>
    screen
      .getByRole("button", { name: "Project Sage" })
      .closest(".project-entry") as HTMLElement;
  expect(titles(sage())).toEqual(["Sage second", "Sage first"]);

  await choose("Sort chats by", "Manual order");
  expect(titles(sage())).toEqual(["Sage first", "Sage second"]);

  await choose("Organize sidebar", "In one list");
  expect(screen.queryByRole("region", { name: "Projects" })).toBeNull();
  expect(titles(screen.getByRole("region", { name: "Recents" }))).toEqual([
    "Cross newest",
    "Sage first",
    "Sage second",
  ]);

  await choose("Sort chats by", "Last updated");
  expect(titles(screen.getByRole("region", { name: "Recents" }))).toEqual([
    "Cross newest",
    "Sage second",
    "Sage first",
  ]);
});

it("promotes pinned tasks without duplicating them in recents", async () => {
  const now = Date.now();
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [
      {
        id: "cross",
        name: "Cross",
        rootCount: 1,
        lastOpenedAt: 2,
        railChats: [
          {
            id: "chat:cross-pinned",
            title: "Pinned research",
            pinned: true,
            position: 0,
            updatedAt: now - 60_000,
          },
          {
            id: "chat:cross-recent",
            title: "Recent research",
            pinned: false,
            position: 1,
            updatedAt: now - 7_200_000,
          },
        ],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:sage",
        kind: "chat",
        title: "Build rail",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
        updatedAt: now - 86_400_000,
      },
    ],
    activeTabId: "chat:sage",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<LeftSidebar />);

  const pinned = screen.getByRole("region", { name: "Pinned" });
  const projects = screen.getByRole("region", { name: "Projects" });
  const recent = screen.getByRole("region", { name: "Recents" });
  expect(
    projects.compareDocumentPosition(pinned) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    pinned.compareDocumentPosition(recent) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(within(pinned).getByText("Pinned research")).toBeInTheDocument();
  expect(within(projects).queryByText("Pinned research")).toBeNull();
  expect(
    await within(recent).findByText("Recent research"),
  ).toBeInTheDocument();
  expect(
    within(recent).getByRole("button", {
      name: "Open recent chat Recent research",
    }),
  ).toBeEnabled();
  expect(within(recent).queryByText("Pinned research")).toBeNull();
  expect(screen.queryByRole("searchbox", { name: "Search tasks" })).toBeNull();
  expect(within(projects).getByText("Recent research")).toBeInTheDocument();
});

it("keeps archive enabled for a project with other attached chats", () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
    tabs: Array.from({ length: 5 }, (_, index) => ({
      id: `chat:${index}`,
      kind: "chat" as const,
      title: index === 4 ? "Stale task" : `Other ${index}`,
      position: index,
      pinned: false,
      dirty: false,
      status: "idle" as const,
    })),
    activeTabId: "chat:0",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);

  fireEvent.click(
    screen.getByRole("button", { name: "Chat actions for Stale task" }),
  );
  expect(screen.getByRole("menuitem", { name: "Archive chat" })).toBeEnabled();
});

it("collapses Pinned and Projects but keeps Recents open", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
    tabs: [
      {
        id: "chat:pinned",
        kind: "chat",
        title: "Pinned task",
        position: 0,
        pinned: true,
        dirty: false,
        status: "idle",
      },
      {
        id: "chat:recent",
        kind: "chat",
        title: "Recent task",
        position: 1,
        pinned: false,
        dirty: false,
        status: "idle",
        updatedAt: Date.now() - 60_000,
      },
    ],
    activeTabId: "chat:recent",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);

  for (const label of ["Pinned", "Projects"]) {
    const disclosure = screen.getByRole("button", { name: label });
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(disclosure);
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
  }
  // Recents is a plain heading, as in Codex: it never collapses.
  expect(
    within(screen.getByRole("region", { name: "Recents" })).getByRole(
      "heading",
      { name: "Recents" },
    ),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: "Recents" })).toBeNull();
});

it("groups attached chats with no listed project under Unknown project", () => {
  useWorkbenchStore.setState({
    project: { id: "missing", name: "Detached", roots: [] },
    projects: [],
    tabs: [
      {
        id: "chat:orphan",
        kind: "chat",
        title: "Unbound task",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:orphan",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);

  const projects = screen.getByRole("region", { name: "Projects" });
  expect(within(projects).getByText("Unknown project")).toBeInTheDocument();
  expect(within(projects).getByText("Unbound task")).toBeInTheDocument();
});

it("shows a labelled running spinner and local project identity without fake account fields", () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
    tabs: [
      {
        id: "chat:run",
        kind: "chat",
        title: "Implement rail",
        position: 0,
        pinned: false,
        dirty: false,
        status: "working",
      },
    ],
    activeTabId: "chat:run",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);

  const row = screen
    .getByRole("button", { name: /^Open chat Implement rail(?:,|$)/ })
    .closest(".project-chat-row");
  expect(
    within(row as HTMLElement).getByRole("img", { name: "Working" }),
  ).toBeInTheDocument();
  const footer = document.querySelector(".project-sidebar__account");
  expect(footer).toHaveTextContent("Sage");
  expect(footer).toHaveTextContent("Local workspace");
  expect(footer?.querySelector(".account-avatar")).toHaveTextContent("S");
  expect(screen.queryByText(/@/)).toBeNull();
  expect(footer?.querySelector(".project-sidebar__settings")).toBeTruthy();
});
