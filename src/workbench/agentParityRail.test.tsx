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
import { resetChatReadMarks } from "../features/chat/acp/chatReadMarks";
import { LeftSidebar } from "./LeftSidebar";
import { resetAgentRailInteractions } from "./agentRailInteractions";
import {
  resetAgentRailViewPrefs,
  setAgentRailOrdering,
  setAgentRailOrganize,
} from "./agentRailViewPrefs";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  resetAgentRailInteractions();
  resetAgentRailViewPrefs();
  resetChatReadMarks();
  useWorkbenchStore.setState(initial, true);
  vi.restoreAllMocks();
});

function seed() {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projects: [{ id: "project-a", name: "A", rootCount: 0, lastOpenedAt: 1 }],
    tabs: ["Alpha", "Beta"].map((title, position) => ({
      id: title,
      kind: "chat",
      title,
      position,
      pinned: false,
      dirty: false,
      status: "complete",
      updatedAt: 100 + position,
    })),
    activeTabId: "Alpha",
    loadRepositoryInventory: vi.fn(async () => undefined),
  });
}

it("collapses projects without activating them and reopens the section from the rail", () => {
  seed();
  const switchProject = vi.fn();
  useWorkbenchStore.setState({ switchProject });
  render(<LeftSidebar />);
  const project = screen.getByRole("button", { name: "Project A" });
  fireEvent.click(project);
  expect(project).toHaveAttribute("aria-expanded", "false");
  expect(switchProject).not.toHaveBeenCalled();
  fireEvent.click(project);
  expect(project).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(screen.getByRole("button", { name: "Projects" }));
  fireEvent.click(screen.getByRole("button", { name: "Go to projects" }));
  expect(screen.getByRole("button", { name: "Projects" })).toHaveAttribute(
    "aria-expanded",
    "true",
  );
  expect(
    screen.queryByRole("button", { name: "Open settings from rail" }),
  ).toBeNull();
  expect(screen.getAllByRole("button", { name: "Search chats" })).toHaveLength(
    1,
  );
});

it("opens the same chat actions by right click and marks a read chat unread", async () => {
  seed();
  render(<LeftSidebar />);
  const projects = screen.getByRole("region", { name: "Projects" });
  const chat = within(projects).getByRole("button", {
    name: "Open chat Alpha",
  });
  fireEvent.contextMenu(chat);
  const menu = await screen.findByRole("menu", { name: "Actions for Alpha" });
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Mark unread" }));
  expect(
    chat
      .closest(".project-chat-row")
      ?.querySelector('[aria-label="Unread updates"]'),
  ).not.toBeNull();
  fireEvent.click(chat);
  expect(
    chat
      .closest(".project-chat-row")
      ?.querySelector('[aria-label="Unread updates"]'),
  ).toBeNull();
});

it("only offers Undo after a committed archive, and restores the exact project/chat", async () => {
  seed();
  const archiveChat = vi.fn(async () => {
    useWorkbenchStore.setState({
      archivedChats: [{ id: "Beta", title: "Beta" }],
      tabs: useWorkbenchStore
        .getState()
        .tabs.filter((tab) => tab.id !== "Beta"),
    });
    return true;
  });
  const restoreChat = vi.fn(async () => true);
  useWorkbenchStore.setState({ archiveChat, restoreChat });
  render(<LeftSidebar />);
  fireEvent.click(
    within(screen.getByRole("region", { name: "Projects" })).getByRole(
      "button",
      { name: "Chat actions for Beta" },
    ),
  );
  fireEvent.click(screen.getByRole("menuitem", { name: "Archive chat" }));
  fireEvent.click(await screen.findByRole("button", { name: "Undo archive" }));
  expect(archiveChat).toHaveBeenCalledWith("Beta", "project-a", "row");
  expect(restoreChat).toHaveBeenCalledWith("Beta", "project-a");
  await act(async () => undefined);
});

it("previews five chats per project and Show more expands the list in place", () => {
  seed();
  useWorkbenchStore.setState({
    tabs: Array.from({ length: 12 }, (_, position) => ({
      id: `chat-${position}`,
      kind: "chat",
      title: `Task ${position}`,
      position,
      pinned: false,
      dirty: false,
      status: "idle",
      updatedAt: position + 1,
    })),
    activeTabId: null,
  });
  render(<LeftSidebar />);
  const rows = () =>
    within(screen.getByRole("region", { name: "Projects" })).getAllByRole(
      "button",
      { name: /^Open chat / },
    );
  expect(rows().map((row) => row.textContent)).toEqual([
    "Task 11",
    "Task 10",
    "Task 9",
    "Task 8",
    "Task 7",
  ]);
  const projects = () =>
    within(screen.getByRole("region", { name: "Projects" }));
  fireEvent.click(projects().getByRole("button", { name: "Show more" }));
  expect(rows()).toHaveLength(12);
  expect(screen.queryByRole("combobox", { name: "Search chats" })).toBeNull();
  fireEvent.click(projects().getByRole("button", { name: "Show less" }));
  expect(rows()).toHaveLength(5);
});

it("does not offer Undo for an archive rejected by the durable store", async () => {
  seed();
  useWorkbenchStore.setState({ archiveChat: vi.fn(async () => false) });
  render(<LeftSidebar />);
  fireEvent.contextMenu(
    within(screen.getByRole("region", { name: "Projects" })).getByRole(
      "button",
      { name: "Open chat Beta" },
    ),
  );
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "Archive chat" }),
  );
  await act(async () => undefined);
  expect(screen.queryByRole("button", { name: "Undo archive" })).toBeNull();
});

it("shows a collapsed project as running when any of its chats runs, even past the preview", () => {
  seed();
  useWorkbenchStore.setState({
    tabs: Array.from({ length: 10 }, (_, position) => ({
      id: `chat-${position}`,
      kind: "chat",
      title: `Task ${position}`,
      position,
      pinned: false,
      dirty: false,
      status: position === 0 ? "working" : "idle",
      updatedAt: position + 1,
    })),
    activeTabId: null,
  });
  render(<LeftSidebar />);
  const header = screen
    .getByRole("button", { name: "Project A" })
    .closest(".project-entry__header") as HTMLElement;
  expect(within(header).queryByRole("img", { name: "Working" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Project A" }));
  expect(within(header).getByRole("img", { name: "Working" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Open chat Task 0" })).toBeNull();
});

it("pins, unpins and archives a chat from its hover actions", async () => {
  seed();
  let refuseArchive = false;
  useWorkbenchStore.setState({
    pinChat: async (chatId: string) =>
      useWorkbenchStore.setState((state) => ({
        tabs: state.tabs.map((tab) =>
          tab.id === chatId ? { ...tab, pinned: !tab.pinned } : tab,
        ),
      })),
    archiveChat: async (chatId: string) => {
      if (refuseArchive) return false;
      useWorkbenchStore.setState((state) => ({
        archivedChats: [{ id: chatId, title: chatId }],
        tabs: state.tabs.filter((tab) => tab.id !== chatId),
      }));
      return true;
    },
  });
  render(<LeftSidebar />);
  const projects = () =>
    within(screen.getByRole("region", { name: "Projects" }));

  fireEvent.click(projects().getByRole("button", { name: "Pin Beta" }));
  const pinned = await screen.findByRole("region", { name: "Pinned" });
  expect(
    within(pinned).getByRole("button", { name: /^Open chat Beta/ }),
  ).toBeVisible();
  expect(
    projects().queryByRole("button", { name: /^Open chat Beta/ }),
  ).toBeNull();

  fireEvent.click(within(pinned).getByRole("button", { name: "Unpin Beta" }));
  expect(
    await projects().findByRole("button", { name: "Open chat Beta" }),
  ).toBeVisible();
  expect(screen.queryByRole("region", { name: "Pinned" })).toBeNull();

  refuseArchive = true;
  fireEvent.click(projects().getByRole("button", { name: "Archive Beta" }));
  await act(async () => undefined);
  expect(
    projects().getByRole("button", { name: "Open chat Beta" }),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: "Undo archive" })).toBeNull();

  refuseArchive = false;
  fireEvent.click(projects().getByRole("button", { name: "Archive Beta" }));
  expect(
    await screen.findByRole("button", { name: "Undo archive" }),
  ).toBeVisible();
  expect(
    projects().queryByRole("button", { name: "Open chat Beta" }),
  ).toBeNull();
});

it("keeps the open chat in view In one list, even far down a manual order", () => {
  seed();
  const tabs = Array.from({ length: 40 }, (_, position) => ({
    id: `chat-${position}`,
    kind: "chat" as const,
    title: `Task ${position}`,
    position,
    pinned: false,
    dirty: false,
    status: "idle" as const,
    updatedAt: 100 - position,
  }));
  useWorkbenchStore.setState({ tabs, activeTabId: "chat-39" });
  act(() => {
    setAgentRailOrganize("list");
    setAgentRailOrdering("manual");
  });
  render(<LeftSidebar />);
  const recents = within(screen.getByRole("region", { name: "Recents" }));
  const open = recents.getByRole("button", { name: "Open chat Task 39" });
  expect(open).toHaveAttribute("aria-current", "page");
  const listed = recents
    .getAllByRole("button", { name: /^Open chat Task / })
    .map((button) => button.getAttribute("aria-label"));
  expect(listed).toHaveLength(40);
  expect(listed[0]).toBe("Open chat Task 0");
  expect(listed[39]).toMatch(/^Open chat Task 39/);
  expect(recents.queryByRole("button", { name: "Show more" })).toBeNull();
});

it("keeps the open chat In one list when it is past the list's cap", () => {
  seed();
  const tabs = Array.from({ length: 64 }, (_, position) => ({
    id: `chat-${position}`,
    kind: "chat" as const,
    title: `Task ${position}`,
    position,
    pinned: false,
    dirty: false,
    status: "idle" as const,
    updatedAt: 1_000 - position,
  }));
  useWorkbenchStore.setState({
    tabs,
    activeTabId: "chat-63",
    projects: [
      { id: "project-a", name: "A", rootCount: 0, lastOpenedAt: 2 },
      {
        id: "project-b",
        name: "B",
        rootCount: 0,
        lastOpenedAt: 1,
        railChats: [
          {
            id: "chat:b",
            title: "Elsewhere",
            pinned: false,
            position: 0,
            updatedAt: 999,
          },
        ],
      },
    ],
  });
  act(() => setAgentRailOrganize("list"));
  render(<LeftSidebar />);
  expect(
    within(screen.getByRole("region", { name: "Recents" })).getByRole(
      "button",
      {
        name: "Open chat Task 63",
      },
    ),
  ).toHaveAttribute("aria-current", "page");
});
