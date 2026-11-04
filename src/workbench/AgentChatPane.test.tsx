import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import {
  chatActivityFingerprint,
  clearUnreadAcks,
  seenChatUnreadFingerprint,
} from "../features/chat/acp/chatUnread";
import { AgentChatPanes } from "./AgentChatPanes";
import { useSideChatState } from "./sideChatState";

const initial = useWorkbenchStore.getState();
const idleSnapshot = {
  chatId: "chat-2",
  statusLabel: "Completed",
  latestText: "Done.",
  streaming: false,
  waiting: false,
  working: false,
  statusTone: "idle" as const,
};
let snapshot: ChatLiveSnapshot = { ...idleSnapshot };
const canvasArgs = vi.fn();
const subscription = vi.fn();
vi.mock("../features/chat/acp/useAcpChatLiveSnapshot", () => ({
  useAcpChatLiveSnapshot: (chatId: string, enabled: boolean) => {
    subscription(chatId, enabled);
    return { ...snapshot, chatId };
  },
}));
vi.mock("./surfaceRegistry", () => ({
  renderRegisteredCanvas: (...args: unknown[]) => {
    canvasArgs(args.length);
    return (
      <button type="button">Focus {(args[1] as WorkbenchTab).title}</button>
    );
  },
}));
const tab: WorkbenchTab = {
  id: "chat-2",
  kind: "chat",
  title: "Second chat",
  position: 1,
  pinned: false,
  dirty: false,
};
beforeEach(() => {
  snapshot = { ...idleSnapshot };
  clearUnreadAcks();
  clearAcpProjectionRegistry();
  canvasArgs.mockClear();
  subscription.mockClear();
  useSideChatState.setState({ sides: [], pairedId: null });
  useWorkbenchStore.setState({ ...initial, tabs: [tab], activeTabId: tab.id });
});
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  clearUnreadAcks();
  useSideChatState.setState({ sides: [], pairedId: null });
  useWorkbenchStore.setState(initial, true);
});

describe("Agent Chat pane through its public panes owner", () => {
  it("renders only the chat canvas, named for its Chat, with no pane header", () => {
    render(<AgentChatPanes />);
    const pane = screen.getByRole("region", { name: "Chat pane Second chat" });
    expect(pane).toHaveAttribute("data-chat-id", "chat-2");
    expect(pane.children).toHaveLength(1);
    expect(
      within(pane).getByRole("button", { name: "Focus Second chat" }),
    ).toBeVisible();
    expect(within(pane).queryByRole("status")).toBeNull();
    expect(within(pane).queryByLabelText("Unread updates")).toBeNull();
    expect(canvasArgs).toHaveBeenCalledWith(2);
  });
  it("acks the current activity while focused", () => {
    render(<AgentChatPanes />);
    expect(seenChatUnreadFingerprint("chat-2")).toBe(
      chatActivityFingerprint(snapshot),
    );
  });
  it("acks the current activity when focus enters an unfocused pane", () => {
    const activateTab = vi.fn(async () => {});
    useWorkbenchStore.setState({
      project: { id: "project", name: "Project", roots: [] },
      tabs: [{ ...tab, id: "chat-1", title: "First chat" }, tab],
      activeTabId: "chat-1",
      activateTab,
    });
    useSideChatState.setState({
      sides: [{ id: "chat-2", projectId: "project", parentChatId: "chat-1" }],
      pairedId: "chat-2",
    });
    render(<AgentChatPanes />);
    expect(seenChatUnreadFingerprint("chat-2")).toBeUndefined();
    fireEvent.focus(screen.getByRole("button", { name: "Focus Second chat" }));
    expect(activateTab).toHaveBeenCalledExactlyOnceWith("chat-2");
    expect(seenChatUnreadFingerprint("chat-2")).toBe(
      chatActivityFingerprint(snapshot),
    );
    expect(screen.getAllByRole("region", { name: /^Chat pane / })).toHaveLength(
      2,
    );
    // The focused owner supplies its snapshot; only the other child subscribes.
    expect(subscription).toHaveBeenCalledWith("chat-1", false);
    expect(subscription).toHaveBeenCalledWith("chat-2", true);
  });
});
