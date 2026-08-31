import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { clearSessionRailAcks } from "../features/chat/acp/sessionRail";
import { ProjectChatNavigation } from "./ProjectChatNavigation";
import {
  resetAgentRailViewPrefs,
  syncAgentRailClock,
} from "./agentRailViewPrefs";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  clearSessionRailAcks();
  resetAgentRailViewPrefs();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initialState, true);
});

function seedChats() {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
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
        status: "working",
      },
      {
        id: "chat:b",
        kind: "chat",
        title: "Beta",
        position: 1,
        pinned: false,
        dirty: false,
        status: "complete",
        updatedAt: Date.UTC(2026, 7, 26, 12, 0, 0),
      },
      {
        id: "chat:c",
        kind: "chat",
        title: "Gamma",
        position: 2,
        pinned: false,
        dirty: false,
        status: "idle",
        updatedAt: Date.UTC(2026, 6, 4, 12, 0, 0),
      },
    ],
    activeTabId: "chat:a",
    activateTab: vi.fn(async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    }),
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
}

function currentChats() {
  return useWorkbenchStore.getState().tabs.filter((tab) => tab.kind === "chat");
}

it("shows Devin session-card status and hides the unread dot on the focused chat", () => {
  syncAgentRailClock(Date.UTC(2026, 7, 26, 18, 0, 0));
  seedChats();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  expect(screen.getByText("Working…")).toBeInTheDocument();
  expect(screen.getByText("Done")).toBeInTheDocument();
  const focused = screen.getByTitle("Open chat Alpha");
  expect(focused).toHaveAttribute("aria-current", "page");
  expect(focused).toHaveAttribute("data-chat-id", "chat:a");
  expect(focused.querySelector("[aria-label='Working']")).toBeInTheDocument();
  expect(
    focused.querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Beta")
      .querySelector("[aria-label='Unread updates']"),
  ).toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Gamma")
      .querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
  expect(screen.getAllByText("feat/onboarding")).toHaveLength(3);
  expect(screen.getByText("6h")).toBeInTheDocument();
  expect(screen.getByText("53d")).toBeInTheDocument();
  expect(screen.getByText("now")).toBeInTheDocument();
  expect(
    screen.getByTitle("Open chat Gamma").querySelector("time"),
  ).toHaveTextContent("53d");
});

it("animates only a live working turn and marks waiting as attention", () => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [
      {
        id: "chat:run",
        kind: "chat",
        title: "Live turn",
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
      {
        id: "chat:idle",
        kind: "chat",
        title: "Idle",
        position: 2,
        pinned: false,
        dirty: false,
        status: "idle",
      },
    ],
    activeTabId: "chat:idle",
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
      attentionMark
    />,
  );

  expect(
    screen
      .getByTitle("Open chat Live turn")
      .querySelector("[aria-label='Working']"),
  ).toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Need a decision")
      .querySelector("[aria-label='Unread updates']"),
  ).toHaveClass("agent-unread-dot--attention");
  expect(
    screen.getByTitle("Open chat Idle").querySelector("[aria-label='Working']"),
  ).toBeNull();
});

it("pins a chat and offers a move popover without Add folder root", () => {
  seedChats();
  const pinChat = vi.fn(async () => undefined);
  const moveChat = vi.fn(async () => undefined);
  useWorkbenchStore.setState({
    projects: [
      { id: "project-a", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
      { id: "project-b", name: "Work", rootCount: 1, lastOpenedAt: 2 },
      { id: "project-local", name: "Local", rootCount: 0, lastOpenedAt: 3 },
    ],
    pinChat,
    moveChat,
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  expect(screen.queryByTitle("Add folder root")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Pin chat Alpha" }));
  expect(pinChat).toHaveBeenCalledWith("chat:a", "project-a");
  fireEvent.click(
    screen.getAllByRole("button", {
      name: "Move chat to another project",
    })[0]!,
  );
  fireEvent.click(screen.getByRole("button", { name: "Move chat to Work" }));
  expect(moveChat).toHaveBeenCalledWith("chat:a", "project-b", "project-a");
});

it("pins and moves a chat that belongs to another project folder", () => {
  seedChats();
  const pinChat = vi.fn(async () => undefined);
  const moveChat = vi.fn(async () => undefined);
  const renameChat = vi.fn(async () => true);
  const archiveChat = vi.fn(async () => true);
  useWorkbenchStore.setState({
    project: { id: "project-sage", name: "Sage", roots: [] },
    projects: [
      { id: "project-a", name: "Cross", rootCount: 1, lastOpenedAt: 1 },
      { id: "project-sage", name: "Sage", rootCount: 1, lastOpenedAt: 2 },
      { id: "project-b", name: "Work", rootCount: 1, lastOpenedAt: 3 },
    ],
    pinChat,
    moveChat,
    renameChat,
    archiveChat,
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={[
        {
          id: "chat:a",
          title: "Alpha",
          pinned: false,
          position: 0,
        },
        {
          id: "chat:extra",
          title: "Extra",
          pinned: false,
          position: 1,
        },
      ]}
      selected={false}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Pin chat Alpha" }));
  expect(pinChat).toHaveBeenCalledWith("chat:a", "project-a");
  fireEvent.click(
    screen.getAllByRole("button", { name: "Move chat to another project" })[0]!,
  );
  fireEvent.click(screen.getByRole("button", { name: "Move chat to Work" }));
  expect(moveChat).toHaveBeenCalledWith("chat:a", "project-b", "project-a");
  fireEvent.click(screen.getByRole("button", { name: "Rename chat Alpha" }));
  fireEvent.change(screen.getByLabelText("New name for Alpha"), {
    target: { value: "Research" },
  });
  fireEvent.submit(
    screen.getByLabelText("New name for Alpha").closest("form")!,
  );
  expect(renameChat).toHaveBeenCalledWith("chat:a", "Research", "project-a");
  fireEvent.click(screen.getByRole("button", { name: "Archive chat Extra" }));
  expect(archiveChat).toHaveBeenCalledWith("chat:extra", "project-a");
});

it("restores an archived chat that belongs to another project folder", () => {
  const restoreChat = vi.fn(async () => true);
  useWorkbenchStore.setState({
    project: { id: "project-sage", name: "Sage", roots: [] },
    archivedChats: [{ id: "chat:local-old", title: "Local old" }],
    restoreChat,
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={[
        {
          id: "chat:seeded",
          title: "Seeded",
          pinned: true,
          position: 0,
        },
      ]}
      archived={[{ id: "chat:extra", title: "Extra" }]}
      selected={false}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "Archived chats" }));
  fireEvent.click(screen.getByRole("button", { name: "Restore chat Extra" }));
  expect(restoreChat).toHaveBeenCalledWith("chat:extra", "project-a");
  expect(
    screen.queryByRole("button", { name: "Restore chat Local old" }),
  ).toBeNull();
  expect(
    screen.getByRole("region", { name: "Archived chats" }),
  ).toHaveAttribute("id", "archived-chat-list-project-a");
});

it("clears the rail unread dot after an unseen Done session is opened", () => {
  seedChats();
  const activateTab = useWorkbenchStore.getState().activateTab;
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  fireEvent.click(screen.getByTitle("Open chat Beta"));
  expect(activateTab).toHaveBeenCalledWith("chat:b");
  expect(
    screen
      .getByTitle("Open chat Beta")
      .querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Alpha")
      .querySelector("[aria-label='Working']"),
  ).toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Alpha")
      .querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
});
