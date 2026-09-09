import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { clearChatSearchFolderRequest } from "../features/chatSearch/chatSearchScope";
import { clearSessionRailAcks } from "../features/chat/acp/sessionRail";
import { LeftSidebar } from "./LeftSidebar";
import {
  resetAgentRailViewPrefs,
  setAgentRailGrouping,
} from "./agentRailViewPrefs";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
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

  const space = screen.getByTitle("Open project Onboarding");
  expect(space).toHaveTextContent("feat/onboarding");
  expect(space.querySelector(".project-row__count")).not.toBeInTheDocument();
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

it("pins and moves a foreign-folder chat without switching first", () => {
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
  fireEvent.click(screen.getByRole("menuitem", { name: "Pin chat" }));
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
  fireEvent.click(screen.getByRole("button", { name: "Archived chats" }));
  fireEvent.click(
    screen.getByRole("button", { name: "Restore chat Research" }),
  );
  expect(restoreChat).toHaveBeenCalledWith("chat:old", "cross");
  expect(switchProject).not.toHaveBeenCalled();
});

it("creates a chat on a foreign folder without switching first", () => {
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
  fireEvent.click(screen.getByRole("button", { name: "New chat in Cross" }));
  expect(createChat).toHaveBeenCalledWith("cross");
  expect(switchProject).not.toHaveBeenCalled();
});

it("opens Chat search on a truncated foreign folder without switching", () => {
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
  expect(screen.getByRole("button", { name: "Search in" })).toHaveTextContent(
    "Cross",
  );
  expect(useWorkbenchStore.getState().project?.id).toBe("sage");
});

it("keeps folder grouping by default and omits Cursor-only rail fields", () => {
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
  expect(
    screen.getByRole("menu", { name: "Chat list options" }),
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
  expect(
    screen.getByRole("region", { name: "Needs Attention" }),
  ).toHaveTextContent("Need a decision");
  expect(screen.getByRole("region", { name: "Working" })).toHaveTextContent(
    "Kickoff",
  );
  expect(screen.getByRole("region", { name: "Done" })).toHaveTextContent(
    "Research agent memory",
  );
  expect(screen.getAllByTitle("Open project Sage").length).toBeGreaterThan(0);
  expect(screen.getByTitle("Open project Cross")).toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Kickoff")
      .closest(".project-chat-row")
      ?.querySelector("[aria-label='Working']"),
  ).toBeInTheDocument();
});

it("marks visible unread rail chats read from the filter menu", () => {
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
  fireEvent.click(screen.getByRole("menuitem", { name: "Mark All as Read" }));
  expect(
    screen
      .getByTitle("Open chat Shipped")
      .closest(".project-chat-row")
      ?.querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
});
