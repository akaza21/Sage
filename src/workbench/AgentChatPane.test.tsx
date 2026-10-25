import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import {
  chatActivityFingerprint,
  clearUnreadAcks,
  seenChatUnreadFingerprint,
} from "../features/chat/acp/chatUnread";
import { AgentChatPane } from "./AgentChatPane";

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

vi.mock("../features/chat/acp/useAcpChatLiveSnapshot", () => ({
  useAcpChatLiveSnapshot: () => snapshot,
}));

vi.mock("./surfaceRegistry", () => ({
  renderRegisteredCanvas: (...args: unknown[]) => {
    canvasArgs(args.length);
    return <button type="button">focused chat</button>;
  },
}));

beforeEach(() => {
  snapshot = { ...idleSnapshot };
  clearUnreadAcks();
  canvasArgs.mockClear();
});

afterEach(cleanup);

const tab: WorkbenchTab = {
  id: "chat-2",
  kind: "chat",
  title: "Second chat",
  position: 1,
  pinned: false,
  dirty: false,
};

describe("AgentChatPane", () => {
  it("renders only the chat canvas, named for its Chat, with no pane header", () => {
    render(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    const pane = screen.getByRole("region", { name: "Chat pane Second chat" });
    expect(pane).toHaveAttribute("data-chat-id", "chat-2");
    expect(pane.children).toHaveLength(1);
    expect(screen.getByRole("button", { name: "focused chat" })).toBeVisible();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
    // The canvas gets the tab alone: no pane header option.
    expect(canvasArgs).toHaveBeenCalledWith(2);
  });

  it("acks the current activity while focused", () => {
    render(
      <AgentChatPane tab={tab} focused={true} onFocus={() => undefined} />,
    );
    expect(seenChatUnreadFingerprint("chat-2")).toBe(
      chatActivityFingerprint(snapshot),
    );
  });

  it("acks the current activity when focus enters an unfocused pane", () => {
    const onFocus = vi.fn();
    render(<AgentChatPane tab={tab} focused={false} onFocus={onFocus} />);
    expect(seenChatUnreadFingerprint("chat-2")).toBeUndefined();
    fireEvent.focus(screen.getByRole("button", { name: "focused chat" }));
    expect(onFocus).toHaveBeenCalledTimes(1);
    expect(seenChatUnreadFingerprint("chat-2")).toBe(
      chatActivityFingerprint(snapshot),
    );
  });
});
