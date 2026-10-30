import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkbenchTab } from "../domain/workbench";
import { chatAttention } from "../services/chatAttention";
import { AgentChatPane } from "./AgentChatPane";

const canvasArgs = vi.fn();

vi.mock("./surfaceRegistry", () => ({
  renderRegisteredCanvas: (...args: unknown[]) => {
    canvasArgs(args.length);
    return <button type="button">focused chat</button>;
  },
}));

beforeEach(() => {
  canvasArgs.mockClear();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

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

  it("pane selection alone does not acknowledge durable attention", () => {
    const ack = vi.spyOn(chatAttention, "ack");
    const onFocus = vi.fn();
    const { rerender } = render(
      <AgentChatPane tab={tab} focused={false} onFocus={onFocus} />,
    );
    fireEvent.focus(screen.getByRole("button", { name: "focused chat" }));
    expect(onFocus).toHaveBeenCalledTimes(1);
    rerender(<AgentChatPane tab={tab} focused onFocus={onFocus} />);
    expect(ack).not.toHaveBeenCalled();
  });
});
