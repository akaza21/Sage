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
import { clearSessionRailAcks } from "../features/chat/acp/sessionRail";
import { LeftSidebar } from "./LeftSidebar";
import { resetAgentRailInteractions } from "./agentRailInteractions";
import {
  agentRailClockMs,
  resetAgentRailViewPrefs,
  syncAgentRailClock,
} from "./agentRailViewPrefs";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  resetAgentRailInteractions();
  resetAgentRailViewPrefs();
  clearSessionRailAcks();
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

it("never moves the rail clock backwards when activity or archive changes", () => {
  syncAgentRailClock(100_000);
  syncAgentRailClock(20_000);
  expect(agentRailClockMs()).toBe(100_000);
});

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
  expect(archiveChat).toHaveBeenCalledWith("Beta", "project-a");
  expect(restoreChat).toHaveBeenCalledWith("Beta", "project-a");
  await act(async () => undefined);
});

it("caps long projects at eight rows and Show more opens the focused project palette", async () => {
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
  });
  render(<LeftSidebar />);
  expect(
    within(screen.getByRole("region", { name: "Projects" })).getAllByRole(
      "button",
      { name: /^Open chat / },
    ),
  ).toHaveLength(8);
  fireEvent.click(screen.getByRole("button", { name: "Show more" }));
  expect(
    await screen.findByRole("searchbox", { name: "Search Chats" }),
  ).toHaveFocus();
  expect(screen.getByRole("button", { name: "Search in" })).toHaveAttribute(
    "data-selected-id",
    "project-a",
  );
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

it("classifies all admitted chats before applying the folder row cap", async () => {
  seed();
  useWorkbenchStore.setState({
    tabs: Array.from({ length: 10 }, (_, position) => ({
      id: `chat-${position}`,
      kind: "chat",
      title: `Task ${position}`,
      position,
      pinned: false,
      dirty: false,
      status: position === 8 ? "waiting" : position === 9 ? "working" : "idle",
    })),
  });
  render(<LeftSidebar />);
  fireEvent.click(screen.getByRole("button", { name: "Chat list options" }));
  fireEvent.click(await screen.findByRole("menuitemradio", { name: "Status" }));
  fireEvent.click(
    await screen.findByRole("menuitemradio", { name: "Needs Attention" }),
  );
  expect(
    screen.getByRole("button", { name: "Open chat Task 8" }),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: "Open chat Task 0" })).toBeNull();
  fireEvent.click(screen.getByRole("menuitemradio", { name: "Working" }));
  expect(
    screen.getByRole("button", { name: "Open chat Task 9" }),
  ).toBeVisible();
});
