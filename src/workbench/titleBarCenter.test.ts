import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { TitleBar } from "./TitleBar";
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
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
  it.each([
    ["Chat: Onboarding", "Onboarding"],
    ["Review PR", "Review PR"],
    [`Chat: ${"n".repeat(120)}`, "n".repeat(120)],
  ])("shows the whole session title: %s", (input, expected) => {
    show([input]);
    expect(screen.getByRole("heading")).toHaveTextContent(expected);
  });
  it.each(["", "   ", "Chat: bad\ntitle"])(
    "rejects empty or control text: %s",
    (title) => {
      show([title]);
      expect(screen.queryByRole("heading")).toBeNull();
    },
  );
  it("uses the active chat or the first remaining chat", () => {
    show(["Chat A", "Chat B"], "chat-1");
    expect(screen.getByRole("heading")).toHaveTextContent("Chat B");
    cleanup();
    show(["Chat A", "Chat B"], "file:app");
    expect(screen.getByRole("heading")).toHaveTextContent("Chat A");
    cleanup();
    show([], "file:app");
    expect(screen.queryByRole("heading")).toBeNull();
  });
});
