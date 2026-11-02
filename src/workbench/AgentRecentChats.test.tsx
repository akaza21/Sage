import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { clearAgentChatOwner, pinAgentChatOwner } from "../app/agentChatOwner";
import {
  ackSessionRailUnread,
  clearSessionRailAcks,
} from "../features/chat/acp/sessionRail";
import { AgentRecentChats } from "./AgentRecentChats";
const initialState = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearSessionRailAcks();
  clearAgentChatOwner();
  useWorkbenchStore.setState(initialState, true);
});

it.each(["chat-current", "diff:current"])(
  "keeps the viewed Recent chat read across a rename with %s active",
  (activeTabId) => {
    useWorkbenchStore.setState({ activeTabId });
    pinAgentChatOwner("chat-current");
    ackSessionRailUnread("chat-current", "Done:100");
    const item = {
      projectId: "project",
      projectName: "Project",
      projectChatCount: 2,
      chat: {
        id: "chat-current",
        title: "Original",
        pinned: false,
        position: 0,
        status: "complete" as const,
        updatedAt: 100,
      },
    };
    const recent = (title: string, updatedAt: number) => (
      <AgentRecentChats
        items={[{ ...item, chat: { ...item.chat, title, updatedAt } }]}
        currentProjectId="project"
        recent
      />
    );
    const view = render(recent("Original", 100));
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
    view.rerender(recent("Renamed", 200));
    expect(
      screen.getByRole("button", { name: "Open recent chat Renamed" }),
    ).toBeVisible();
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
    act(() => {
      useWorkbenchStore.setState({ activeTabId: "chat-other" });
      pinAgentChatOwner("chat-other");
    });
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
    // New activity while away still produces an unread indicator.
    view.rerender(recent("Renamed", 300));
    expect(screen.getByLabelText("Unread updates")).toBeVisible();
  },
);
it("renders eight recent tasks and expands only on an explicit request", () => {
  const items = Array.from({ length: 20 }, (_, index) => ({
    projectId: "project",
    projectName: "Project",
    projectChatCount: 20,
    chat: {
      id: `chat-${index}`,
      title: `Task ${index}`,
      pinned: false,
      position: index,
      updatedAt: 100,
    },
  }));
  render(<AgentRecentChats items={items} currentProjectId="project" recent />);
  expect(
    screen.getAllByRole("button", { name: /^Open recent chat Task / }),
  ).toHaveLength(8);
  fireEvent.click(screen.getByRole("button", { name: "Show more" }));
  expect(
    screen.getAllByRole("button", { name: /^Open recent chat Task / }),
  ).toHaveLength(16);
  fireEvent.click(screen.getByRole("button", { name: "Show more" }));
  expect(
    screen.getAllByRole("button", { name: /^Open recent chat Task / }),
  ).toHaveLength(20);
  expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
});
