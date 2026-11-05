import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { TitleBar } from "./TitleBar";
import {
  clearAgentChatOwner,
  pinAgentChatOwner,
  releaseAgentChatOwner,
} from "../app/agentChatOwner";
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAgentChatOwner();
  useWorkbenchStore.setState(initial, true);
});
function show(titles: string[], activeTabId: string | null = "chat-0") {
  useWorkbenchStore.setState({
    tabs: titles.map((title, index) => ({
      id: `chat-${index}`,
      kind: "chat",
      title,
      position: index,
      pinned: false,
      dirty: false,
    })),
    activeTabId,
  });
  render(createElement(TitleBar, { mode: "agent", onModeChange: () => {} }));
}
describe("title bar center labels", () => {
  it("keeps window chrome clear while a Chat owns a resource or Settings", () => {
    pinAgentChatOwner("chat-1");
    show(["Chat A", "Chat B"], "browser-1");
    expect(screen.queryByRole("heading")).toBeNull();
    useWorkbenchStore.setState({ activeTabId: "settings" });
    releaseAgentChatOwner();
    expect(screen.queryByRole("heading")).toBeNull();
  });
  it.each([
    ["Chat: Onboarding", "Onboarding"],
    ["Review PR", "Review PR"],
    [`Chat: ${"n".repeat(50)}`, `${"n".repeat(47)}…`],
  ])(
    "does not duplicate a session title in window chrome: %s",
    (input, expected) => {
      show([input]);
      expect(screen.queryByText(expected)).toBeNull();
      expect(screen.queryByRole("heading")).toBeNull();
    },
  );
  it.each(["", "   ", "Chat: bad\ntitle"])(
    "rejects empty or control text: %s",
    (title) => {
      show([title]);
      expect(screen.queryByRole("heading")).toBeNull();
    },
  );
  it("keeps the window title clear when the active Chat changes or closes", () => {
    show(["Chat A", "Chat B"], "chat-1");
    expect(screen.queryByRole("heading")).toBeNull();
    cleanup();
    show(["Chat A", "Chat B"], "file:app");
    expect(screen.queryByRole("heading")).toBeNull();
    cleanup();
    show([], "file:app");
    expect(screen.queryByRole("heading")).toBeNull();
  });
});
