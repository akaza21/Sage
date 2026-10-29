import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import {
  clearChatSearchFolderRequest,
  hasChatSearchFolderRequest,
  requestChatSearchInFolder,
} from "../features/chatSearch/chatSearchScope";
import { clearSessionRailAcks } from "../features/chat/acp/sessionRail";
import { LeftSidebar } from "./LeftSidebar";
import { resetAgentRailInteractions } from "./agentRailInteractions";
import {
  resetAgentRailViewPrefs,
  setAgentRailGrouping,
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
    ).getByRole("button", { name: "New chat" }),
  );
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
  expect(useAgentSettingsState.getState().page).toBe("General");
});

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  resetAgentRailInteractions();
  useAgentSettingsState.getState().close();
  clearChatSearchFolderRequest();
  clearSessionRailAcks();
  resetAgentRailViewPrefs();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

it("shows a Devin space-header branch chip on the selected project", () => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Onboarding", roots: [] },
    projects: [
      { id: "project-a", name: "Onboarding", rootCount: 1, lastOpenedAt: 1 },
    ],
    repositories: {
      roots: [
        {
          rootId: "root-a",
          state: "repository",
          branch: "feat/onboarding",
          changes: [],
          truncated: false,
          elapsedMs: 1,
          message: null,
        },
      ],
      truncated: false,
      elapsedMs: 1,
    },
    tabs: [
      {
        id: "chat:a",
        kind: "chat",
        title: "Alpha",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:a",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });

  render(<LeftSidebar />);

  const space = screen.getByTitle("Project Onboarding");
  expect(space).toHaveTextContent("feat/onboarding");
  expect(space.querySelector(".project-row__count")).not.toBeInTheDocument();
});

it("labels the New task action with its keyboard chord", () => {
  render(<LeftSidebar />);
  const button = screen.getByRole("button", { name: "New task" });
  expect(button).toHaveTextContent("Ctrl+N");
  expect(button).toHaveAttribute("aria-keyshortcuts", "Control+N");
});

it("keeps New task available when no folder project is open", () => {
  const createChat = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    project: null,
    projects: [],
    createChat,
  });
  render(<LeftSidebar />);
  const button = screen.getByRole("button", { name: "New task" });
  expect(button).toBeEnabled();
  expect(
    screen.getByText(
      "New chat starts under Local. Open a folder when you want files.",
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
    screen.getByTitle("Open chat Continue prior chat goal"),
  ).toBeInTheDocument();
  expect(
    screen.getByTitle("Open chat Research agent memory"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByTitle("Open chat Research agent memory"));
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
  expect(restoreChat).toHaveBeenCalledWith("chat:old", "cross");
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

it("keeps folder grouping by default and omits Cursor-only rail fields", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
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
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  expect(screen.getByText("Projects")).toBeInTheDocument();
  expect(screen.queryByRole("menuitemradio", { name: "Status" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Chat list options" }));
  // The open menu loads lazily on its first open.
  expect(
    await screen.findByRole("menu", { name: "Chat list options" }),
  ).toHaveTextContent("Updated");
  expect(
    screen.getByRole("menuitemcheckbox", { name: "Workspace" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("menuitemcheckbox", { name: "Branch" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("menuitemcheckbox", { name: "Archived" }),
  ).toBeInTheDocument();
  expect(screen.queryByText("Environment")).toBeNull();
  expect(screen.queryByText("PR")).toBeNull();
  expect(screen.queryByText("Machine")).toBeNull();
  expect(screen.queryByText("Source")).toBeNull();
  expect(screen.queryByText("Automations")).toBeNull();
  expect(screen.queryByText("Customize")).toBeNull();
});

it("groups chats by durable status without dropping folder identity", () => {
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
            status: "idle",
          },
        ],
      },
      { id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
    ],
    tabs: [
      {
        id: "chat:run",
        kind: "chat",
        title: "Kickoff",
        position: 0,
        pinned: false,
        dirty: false,
        status: "working",
      },
      {
        id: "chat:wait",
        kind: "chat",
        title: "Need a decision",
        position: 1,
        pinned: false,
        dirty: false,
        status: "waiting",
      },
    ],
    activeTabId: "chat:run",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  setAgentRailGrouping("status");
  render(<LeftSidebar />);
  expect(screen.getByText("Chats")).toBeInTheDocument();
  const scrollToChats = vi.fn();
  screen.getByRole("region", { name: "Chats" }).scrollIntoView = scrollToChats;
  fireEvent.click(screen.getByRole("button", { name: "Go to projects" }));
  expect(scrollToChats).toHaveBeenCalledTimes(1);
  expect(
    screen.getByRole("region", { name: "Needs Attention" }),
  ).toHaveTextContent("Need a decision");
  expect(screen.getByRole("region", { name: "Working" })).toHaveTextContent(
    "Kickoff",
  );
  expect(screen.getByRole("region", { name: "Done" })).toHaveTextContent(
    "Research agent memory",
  );
  expect(screen.getAllByTitle("Project Sage").length).toBeGreaterThan(0);
  expect(screen.getByTitle("Project Cross")).toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Kickoff")
      .closest(".project-chat-row")
      ?.querySelector(".project-chat__status-chip--working"),
  ).toBeInTheDocument();
});

it("promotes pinned tasks while keeping all timestamped chats in recents", () => {
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
  const recent = screen.getByRole("region", { name: "Recent" });
  expect(
    projects.compareDocumentPosition(pinned) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    pinned.compareDocumentPosition(recent) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(within(pinned).getByText("Pinned research")).toBeInTheDocument();
  expect(within(projects).queryByText("Pinned research")).toBeNull();
  expect(within(recent).getByText("Recent research")).toBeInTheDocument();
  expect(within(recent).getByText("2h ago")).toBeInTheDocument();
  expect(
    within(recent).getByRole("button", {
      name: "Open recent chat Recent research",
    }),
  ).toBeEnabled();
  expect(within(recent).getByText("Pinned research")).toBeInTheDocument();
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

it("collapses Pinned, Projects, and Recent through the shared preference set", async () => {
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

  for (const label of ["Pinned", "Projects", "Recent"]) {
    const disclosure = screen.getByRole("button", { name: label });
    expect(disclosure).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(disclosure);
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
  }

  fireEvent.click(screen.getByRole("button", { name: "Projects" }));
  fireEvent.click(screen.getByRole("button", { name: "Chat list options" }));
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Collapse All" }),
  );
  expect(screen.getByRole("button", { name: "Pinned" })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
  expect(screen.getByRole("button", { name: "Projects" })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
  expect(screen.getByRole("button", { name: "Recent" })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
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

it("shows labelled active status and local project identity without fake account fields", () => {
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
    .getByTitle("Open chat Implement rail")
    .closest(".project-chat-row");
  expect(row?.querySelector(".project-chat__status-chip")).toHaveTextContent(
    "Working…",
  );
  const footer = document.querySelector(".project-sidebar__account");
  expect(footer).toHaveTextContent("Sage");
  expect(footer).toHaveTextContent("Local workspace");
  expect(footer?.querySelector(".account-avatar")).toHaveTextContent("S");
  expect(screen.queryByText(/@/)).toBeNull();
  expect(footer?.querySelector(".project-sidebar__settings")).toBeTruthy();
});

it("marks visible unread rail chats read from the filter menu", async () => {
  useWorkbenchStore.setState({
    project: { id: "sage", name: "Sage", roots: [] },
    projects: [{ id: "sage", name: "Sage", rootCount: 1, lastOpenedAt: 1 }],
    tabs: [
      {
        id: "chat:done",
        kind: "chat",
        title: "Shipped",
        position: 0,
        pinned: false,
        dirty: false,
        status: "complete",
      },
      {
        id: "chat:focus",
        kind: "chat",
        title: "Focus",
        position: 1,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:focus",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
  render(<LeftSidebar />);
  expect(
    screen
      .getByTitle("Open chat Shipped")
      .closest(".project-chat-row")
      ?.querySelector("[aria-label='Unread updates']"),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Chat list options" }));
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Mark All as Read" }),
  );
  expect(
    screen
      .getByTitle("Open chat Shipped")
      .closest(".project-chat-row")
      ?.querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
});
