import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { clearUnreadAcks } from "../features/chat/acp/chatUnread";
import { AgentChatPane } from "./AgentChatPane";

const liveSnapshot = {
  chatId: "chat-2",
  statusLabel: "Running",
  latestText: "Working on the diff",
  streaming: true,
  waiting: false,
  working: true,
  statusTone: "working" as const,
};

const idleSnapshot = {
  chatId: "chat-2",
  statusLabel: "Completed",
  latestText: "Done.",
  streaming: false,
  waiting: false,
  working: false,
  statusTone: "idle" as const,
};

const neverStartedSnapshot = {
  chatId: "chat-2",
  statusLabel: "Idle",
  latestText: "",
  streaming: false,
  waiting: false,
  working: false,
  statusTone: "idle" as const,
};

const failedSnapshot = {
  chatId: "chat-2",
  statusLabel: "Failed",
  latestText: "The turn failed.",
  streaming: false,
  waiting: false,
  working: false,
  statusTone: "failed" as const,
};

let snapshot: ChatLiveSnapshot = { ...liveSnapshot };

vi.mock("../features/chat/acp/useAcpChatLiveSnapshot", () => ({
  useAcpChatLiveSnapshot: () => snapshot,
}));

vi.mock("./surfaceRegistry", () => ({
  renderRegisteredCanvas: () => <div>focused chat</div>,
}));

beforeEach(() => {
  snapshot = { ...liveSnapshot };
  clearUnreadAcks();
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

describe("AgentChatPane unread dot", () => {
  it("marks an unfocused live pane as unread while keeping the chat canvas mounted", () => {
    render(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.getByLabelText("Unread updates")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Running");
    expect(screen.getByText("focused chat")).toBeInTheDocument();
    expect(screen.queryByText("Working on the diff")).toBeNull();
    expect(screen.getByText("Second chat")).toHaveClass(
      "agent-chat-pane__title",
    );
  });

  it("projects failed status through a stable semantic tone", () => {
    snapshot = { ...failedSnapshot };
    render(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.getByRole("status")).toHaveClass("agent-chat-chip--failed");
  });

  it("hides the unread dot on an unfocused never-started idle pane", () => {
    snapshot = { ...neverStartedSnapshot };
    render(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Idle");
  });

  it("keeps the unread dot on an unfocused idle pane until that fingerprint is seen", () => {
    snapshot = { ...idleSnapshot };
    render(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.getByLabelText("Unread updates")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Completed");
  });

  it("hides the unread dot once the pane is focused", () => {
    render(
      <AgentChatPane tab={tab} focused={true} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
    expect(screen.getByText("focused chat")).toBeInTheDocument();
  });

  it("does not mark a pane unread after a turn completes while that pane stays focused", () => {
    const { rerender } = render(
      <AgentChatPane tab={tab} focused={true} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();

    snapshot = { ...idleSnapshot };
    rerender(
      <AgentChatPane tab={tab} focused={true} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();

    rerender(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
  });

  it("acks the current fingerprint from the header focus click so idle unread stays cleared", () => {
    snapshot = { ...idleSnapshot };
    const { rerender } = render(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.getByLabelText("Unread updates")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Second chat/ }));
    rerender(
      <AgentChatPane tab={tab} focused={true} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();

    rerender(
      <AgentChatPane tab={tab} focused={false} onFocus={() => undefined} />,
    );
    expect(screen.queryByLabelText("Unread updates")).toBeNull();
  });
});
