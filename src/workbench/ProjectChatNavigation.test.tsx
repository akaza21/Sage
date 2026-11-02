import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { clearSessionRailAcks } from "../features/chat/acp/sessionRail";
import { setChatAwaitingApproval } from "../features/chat/acp/awaitingApproval";
import { ProjectChatNavigation } from "./ProjectChatNavigation";
import { useSideChatState } from "./sideChatState";
import { mirrorAgentRailStatus } from "./useAgentRailProjection";
import type { SageChatId, SageProjectId } from "../domain/acp";
import {
  resetAgentRailViewPrefs,
  syncAgentRailClock,
} from "./agentRailViewPrefs";

const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  clearSessionRailAcks();
  resetAgentRailViewPrefs();
  useSideChatState.setState({ sides: [] });
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

it("shows a green Awaiting approval pill only for a chat whose surface awaits approval", () => {
  seedChats();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );
  expect(screen.queryByText("Awaiting approval")).toBeNull();
  act(() => setChatAwaitingApproval("chat:b", true));
  const pill = screen.getByText("Awaiting approval");
  expect(pill).toHaveClass("rail-pill", "rail-pill--approval");
  expect(screen.getByTitle("Open chat Beta")).toContainElement(pill);
  expect(screen.getByTitle("Open chat Beta")).toHaveAccessibleName(
    "Open chat Beta, Awaiting approval",
  );
  expect(screen.getAllByText("Awaiting approval")).toHaveLength(1);
  // The Turn still runs behind the pill, so its spinner follows (s6/f0526).
  expect(
    screen
      .getByTitle("Open chat Beta")
      .parentElement?.querySelector(".project-chat__approval-spinner .spinner"),
  ).not.toBeNull();
  act(() => setChatAwaitingApproval("chat:b", false));
  expect(screen.queryByText("Awaiting approval")).toBeNull();
  expect(document.querySelector(".project-chat__approval-spinner")).toBeNull();
});

it("keeps the parent row selected while its side chat has focus (Codex s2/f0244)", () => {
  seedChats();
  useWorkbenchStore.setState({ activeTabId: "side-1" });
  useSideChatState.setState({
    sides: [{ id: "side-1", projectId: "project-a", parentChatId: "chat:b" }],
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );
  expect(screen.getByTitle("Open chat Beta")).toHaveAttribute(
    "aria-current",
    "page",
  );
  expect(screen.getByTitle("Open chat Alpha")).not.toHaveAttribute(
    "aria-current",
  );
});

async function openChatActions(title: string) {
  fireEvent.click(
    screen.getByRole("button", { name: `Chat actions for ${title}` }),
  );
  await screen.findByRole("menu", { name: `Actions for ${title}` });
}

it("shows one prioritized trailing state per chat and keeps actions separate", () => {
  syncAgentRailClock(Date.UTC(2026, 7, 26, 18, 0, 0));
  seedChats();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  const focused = screen.getByTitle("Open chat Alpha");
  expect(focused).toHaveAttribute("aria-current", "page");
  expect(focused).toHaveAttribute("data-chat-id", "chat:a");
  const focusedRow = focused.closest(".project-chat-row");
  expect(focusedRow).toHaveClass("project-chat-row--active");
  expect(focusedRow?.querySelectorAll(".project-chat__state")).toHaveLength(1);
  expect(focusedRow?.querySelector(".project-chat__state")).toHaveTextContent(
    "Working…",
  );
  const betaRow = screen
    .getByTitle("Open chat Beta")
    .closest(".project-chat-row");
  expect(betaRow).not.toHaveClass("project-chat-row--active");
  expect(betaRow?.querySelectorAll(".project-chat__state")).toHaveLength(1);
  expect(betaRow?.querySelector(".project-chat__state")).toHaveTextContent(
    "Unread",
  );
  const gammaRow = screen
    .getByTitle("Open chat Gamma")
    .closest(".project-chat-row");
  expect(gammaRow?.querySelectorAll(".project-chat__state")).toHaveLength(1);
  expect(gammaRow?.querySelector(".project-chat__state")).toHaveTextContent(
    "53d",
  );
  expect(screen.getAllByText("feat/onboarding")).toHaveLength(3);
  expect(gammaRow?.querySelector("time")).toHaveTextContent("53d");
  expect(focusedRow?.querySelector(".project-chat__trailing")).toContainElement(
    screen.getByRole("button", { name: "Chat actions for Alpha" }),
  );
});

it("suffixes only numeric recent ages", () => {
  syncAgentRailClock(Date.UTC(2026, 7, 26, 18, 0, 0));
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [],
    activeTabId: null,
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={[
        {
          id: "chat:now",
          title: "Now",
          position: 0,
          pinned: false,
          status: "idle",
          updatedAt: Date.UTC(2026, 7, 26, 17, 59, 30),
        },
        {
          id: "chat:yesterday",
          title: "Yesterday",
          position: 1,
          pinned: false,
          status: "idle",
          updatedAt: Date.UTC(2026, 7, 25, 12, 0, 0),
        },
        {
          id: "chat:hours",
          title: "Hours",
          position: 2,
          pinned: false,
          status: "idle",
          updatedAt: Date.UTC(2026, 7, 26, 16, 0, 0),
        },
        {
          id: "chat:capped",
          title: "Capped",
          position: 3,
          pinned: false,
          status: "idle",
          updatedAt: Date.UTC(2026, 3, 1, 18, 0, 0),
        },
      ]}
      selected
      relativeTimeSuffix
      suppressActive
    />,
  );

  expect(screen.getByText("now")).toBeInTheDocument();
  expect(screen.getByText("yesterday")).toBeInTheDocument();
  expect(screen.getByText("2h ago")).toBeInTheDocument();
  expect(screen.getByText("99d+ ago")).toBeInTheDocument();
  expect(screen.queryByText("now ago")).toBeNull();
  expect(screen.queryByText("yesterday ago")).toBeNull();
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
    />,
  );

  expect(
    screen
      .getByTitle("Open chat Live turn")
      .closest(".project-chat-row")
      ?.querySelector(".project-chat__state"),
  ).toHaveTextContent("Working…");
  expect(
    screen
      .getByTitle("Open chat Need a decision")
      .closest(".project-chat-row")
      ?.querySelector(".project-chat__state"),
  ).toHaveTextContent("Waiting");
  expect(
    screen
      .getByTitle("Open chat Idle")
      .closest(".project-chat-row")
      ?.querySelector(".project-chat__state"),
  ).toBeNull();
});

it("keeps pin state visible and restores focus from one chat actions menu", async () => {
  seedChats();
  useWorkbenchStore.setState({
    tabs: currentChats().map((tab) =>
      tab.id === "chat:a" ? { ...tab, pinned: true } : tab,
    ),
  });
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  expect(
    screen.getByRole("button", { name: "Open chat Alpha, pinned" }),
  ).toContainElement(document.querySelector(".project-chat__pinned"));
  expect(screen.queryByRole("button", { name: "Unpin chat Alpha" })).toBeNull();
  const actionTrigger = screen.getByRole("button", {
    name: "Chat actions for Alpha",
  });
  fireEvent.click(actionTrigger);
  expect(
    await screen.findByRole("menu", { name: "Actions for Alpha" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("menuitem", { name: "Unpin chat" }),
  ).toBeInTheDocument();

  fireEvent.keyDown(screen.getByRole("menu", { name: "Actions for Alpha" }), {
    key: "Escape",
  });
  expect(screen.queryByRole("menu", { name: "Actions for Alpha" })).toBeNull();
  expect(actionTrigger).toHaveFocus();
});

it("roves through chat actions and dismisses the menu on Tab", async () => {
  seedChats();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  expect(document.querySelector('[role="menu"]')).toBeNull();
  await openChatActions("Alpha");
  const menu = screen.getByRole("menu", { name: "Actions for Alpha" });
  const pin = screen.getByRole("menuitem", { name: "Pin chat" });
  const move = screen.getByRole("menuitem", { name: "Move to project" });
  const rename = screen.getByRole("menuitem", { name: "Rename chat" });
  const archive = screen.getByRole("menuitem", { name: "Archive chat" });

  expect(pin).toHaveFocus();
  expect(pin).toHaveAttribute("tabindex", "0");
  expect(move).toHaveAttribute("tabindex", "-1");
  expect(rename).toHaveAttribute("tabindex", "-1");

  fireEvent.keyDown(menu, { key: "ArrowDown" });
  expect(move).toHaveFocus();
  fireEvent.keyDown(menu, { key: "End" });
  expect(archive).toHaveFocus();
  fireEvent.keyDown(menu, { key: "Tab" });

  expect(screen.queryByRole("menu", { name: "Actions for Alpha" })).toBeNull();
  expect(document.querySelector('[role="menu"]')).toBeNull();
});

it("dismisses chat actions on an outside pointer without stealing focus", async () => {
  seedChats();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );

  await openChatActions("Alpha");
  const outside = screen.getByTitle("Open chat Beta");
  outside.focus();
  fireEvent.pointerDown(outside);

  expect(screen.queryByRole("menu", { name: "Actions for Alpha" })).toBeNull();
  expect(outside).toHaveFocus();
});

it("pins a chat and offers a move popover without Add folder root", async () => {
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
  await openChatActions("Alpha");
  fireEvent.click(screen.getByRole("menuitem", { name: "Pin chat" }));
  expect(pinChat).toHaveBeenCalledWith("chat:a", "project-a");
  await openChatActions("Alpha");
  fireEvent.click(screen.getByRole("menuitem", { name: "Move to project" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Move chat to Work" }));
  expect(moveChat).toHaveBeenCalledWith("chat:a", "project-b", "project-a");
});

it("pins and moves a chat that belongs to another project folder", async () => {
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

  await openChatActions("Alpha");
  fireEvent.click(screen.getByRole("menuitem", { name: "Pin chat" }));
  expect(pinChat).toHaveBeenCalledWith("chat:a", "project-a");
  await openChatActions("Alpha");
  fireEvent.click(screen.getByRole("menuitem", { name: "Move to project" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Move chat to Work" }));
  expect(moveChat).toHaveBeenCalledWith("chat:a", "project-b", "project-a");
  await openChatActions("Alpha");
  fireEvent.click(screen.getByRole("menuitem", { name: "Rename chat" }));
  const renameInput = await screen.findByLabelText("New name for Alpha");
  expect(renameInput).toHaveFocus();
  fireEvent.change(renameInput, {
    target: { value: "Research" },
  });
  fireEvent.submit(
    screen.getByLabelText("New name for Alpha").closest("form")!,
  );
  expect(renameChat).toHaveBeenCalledWith("chat:a", "Research", "project-a");
  await openChatActions("Extra");
  fireEvent.click(screen.getByRole("menuitem", { name: "Archive chat" }));
  expect(archiveChat).toHaveBeenCalledWith("chat:extra", "project-a");
});

it("labels the archived count separately and preserves disclosure semantics", () => {
  useWorkbenchStore.setState({
    archivedChats: [{ id: "chat:old", title: "Research" }],
  });
  render(<ProjectChatNavigation projectId="project-a" chats={[]} selected />);
  const disclosure = screen.getByRole("button", { name: /^Archived chats:/ });
  expect(disclosure).toHaveTextContent("Archived 1");
  expect(disclosure).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(disclosure);
  expect(disclosure).toHaveAttribute("aria-expanded", "true");
  expect(
    screen.getByRole("region", { name: "Archived chats" }),
  ).toHaveAttribute("id", disclosure.getAttribute("aria-controls"));
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

  fireEvent.click(screen.getByRole("button", { name: /^Archived chats:/ }));
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
      .closest(".project-chat-row")
      ?.querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Alpha")
      .closest(".project-chat-row")
      ?.querySelector(".project-chat__state--working"),
  ).toBeInTheDocument();
  expect(
    screen
      .getByTitle("Open chat Alpha")
      .closest(".project-chat-row")
      ?.querySelector("[aria-label='Unread updates']"),
  ).not.toBeInTheDocument();
});

it("saves a viewed Done chat as read and trusts that mark after a reload", () => {
  seedChats();
  const saved = vi
    .spyOn(getWorkbenchPorts().chat, "markChatViewed")
    .mockResolvedValue();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );
  fireEvent.click(screen.getByTitle("Open chat Beta"));
  expect(saved).toHaveBeenCalledWith({
    projectId: "project-a",
    chatId: "chat:b",
  });

  // A reload starts with no seen fingerprints; the tabs come from native
  // with the viewed mark, and another chat has focus.
  cleanup();
  clearSessionRailAcks();
  seedChats();
  useWorkbenchStore.setState((state) => ({
    tabs: state.tabs.map((tab) =>
      tab.id === "chat:b" ? { ...tab, viewed: true } : tab,
    ),
  }));
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );
  const betaRow = () =>
    screen.getByTitle("Open chat Beta").closest(".project-chat-row");
  expect(betaRow()).not.toHaveTextContent("Unread");

  // A turn that finishes later makes the loaded mark stale.
  act(() => {
    mirrorAgentRailStatus({
      projectId: "project-a" as SageProjectId,
      chatId: "chat:b" as SageChatId,
      title: "Beta",
      latestTurn: {
        state: "completed",
        createdAt: Date.UTC(2026, 7, 27, 12, 0, 0),
        terminalAt: Date.UTC(2026, 7, 27, 12, 1, 0),
        updatedAt: Date.UTC(2026, 7, 27, 12, 1, 0),
      },
    });
  });
  cleanup();
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={currentChats()}
      selected
    />,
  );
  expect(betaRow()).toHaveTextContent("Unread");
});
