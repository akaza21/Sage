import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import type { AcpChatPage } from "../domain/acp";
import type { WorkbenchTab } from "../domain/workbench";
import { page as projectionPage } from "../features/chat/acp/acpProjection.fixture";
import { acpDesktop } from "../services/acpDesktop";
import {
  chatActivityFingerprint,
  clearUnreadAcks,
  seenChatUnreadFingerprint,
} from "../features/chat/acp/chatUnread";
import { AgentChatPanes } from "./AgentChatPanes";
import { useSideChatState } from "./sideChatState";

const initial = useWorkbenchStore.getState();
const snapshot = {
  chatId: "chat-2",
  statusLabel: "Completed",
  latestText: "Done.",
  streaming: false,
  waiting: false,
  working: false,
  statusTone: "idle" as const,
};
const pages = new Map<string, AcpChatPage>();
vi.mock("./surfaceRegistry", () => ({
  renderRegisteredCanvas: (_kind: string, tab: WorkbenchTab) => (
    <button type="button">Focus {tab.title}</button>
  ),
}));
const tab: WorkbenchTab = {
  id: "chat-2",
  kind: "chat",
  title: "Second chat",
  position: 1,
  pinned: false,
  dirty: false,
};
function seedProjection(chatId: string) {
  const fixture = projectionPage("completed");
  const page: AcpChatPage = {
    ...fixture,
    chatId: chatId as AcpChatPage["chatId"],
    turns: fixture.turns.map((turn) => ({
      ...turn,
      items: turn.items.map((item) =>
        item.kind === "text"
          ? {
              ...item,
              ordinal: 0,
              lifecycle: "completed" as const,
              payload: { ...item.payload, text: "Done." },
            }
          : item,
      ),
    })),
  };
  pages.set(chatId, page);
  const store = acpProjectionStoreFor(chatId);
  const authority = {
    projectId: page.projectId,
    projectGeneration: "1",
    chatId: page.chatId,
  } as never;
  store.getState().activateScope(authority);
  store.getState().applyAuthoritativePage(authority, page);
}
beforeEach(() => {
  clearUnreadAcks();
  clearAcpProjectionRegistry();
  pages.clear();
  useSideChatState.setState({ sides: [], pairedId: null });
  useWorkbenchStore.setState({
    ...initial,
    project: { id: "project-1", name: "Project", roots: [] },
    projectGeneration: 1,
    nativeTransitionEpoch: 1,
    projectTransitioning: false,
    tabs: [tab],
    activeTabId: tab.id,
  });
  seedProjection(tab.id);
  vi.spyOn(acpDesktop, "getChatPage").mockImplementation(async (request) =>
    pages.get(request.chatId)!,
  );
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
    () => undefined,
  );
  vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockResolvedValue(
    () => undefined,
  );
});
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  clearUnreadAcks();
  vi.restoreAllMocks();
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
    expect(screen.getAllByRole("region", { name: /^Chat pane / })).toHaveLength(
      1,
    );
  });
  it("acks the current activity while focused", () => {
    render(<AgentChatPanes />);
    expect(seenChatUnreadFingerprint("chat-2")).toBe(
      chatActivityFingerprint(snapshot),
    );
  });
  it("acks the current activity when focus enters an unfocused pane", () => {
    useWorkbenchStore.setState({
      tabs: [{ ...tab, id: "chat-1", title: "First chat" }, tab],
      activeTabId: "chat-1",
      activateTab: async (tabId) => {
        useWorkbenchStore.setState({ activeTabId: tabId });
      },
    });
    seedProjection("chat-1");
    useSideChatState.setState({
      sides: [{ id: "chat-2", projectId: "project-1", parentChatId: "chat-1" }],
      pairedId: "chat-2",
    });
    render(<AgentChatPanes />);
    const parentSeen = seenChatUnreadFingerprint("chat-1");
    expect(seenChatUnreadFingerprint("chat-2")).toBeUndefined();
    fireEvent.focus(screen.getByRole("button", { name: "Focus Second chat" }));
    expect(useWorkbenchStore.getState().activeTabId).toBe("chat-2");
    expect(seenChatUnreadFingerprint("chat-2")).toBe(
      chatActivityFingerprint(snapshot),
    );
    expect(seenChatUnreadFingerprint("chat-1")).toBe(parentSeen);
    expect(screen.getAllByRole("region", { name: /^Chat pane / })).toHaveLength(
      2,
    );
  });
});
