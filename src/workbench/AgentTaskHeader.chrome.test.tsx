import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { AgentTaskStatus } from "./AgentTaskStatus";

const initialState = useWorkbenchStore.getState();

it.each([
  ["waiting", "lucide-triangle-alert"],
  ["failed", "lucide-circle-alert"],
  ["working", null],
  ["idle", null],
] as const)(
  "keeps the %s status glyph and accessible label",
  (tone, iconClass) => {
    render(
      <AgentTaskStatus
        snapshot={{
          chatId: "chat:chrome",
          statusLabel: tone,
          statusTone: tone,
          latestText: "",
          streaming: false,
          waiting: tone === "waiting",
          working: tone === "working",
        }}
      />,
    );
    const status = screen.getByRole("status", { name: `Chat status: ${tone}` });
    expect(status).toHaveAttribute("title", tone);
    expect(status).toHaveTextContent(tone);
    const icons = status.querySelectorAll("svg");
    expect(icons).toHaveLength(iconClass ? 1 : 0);
    if (iconClass) {
      expect(icons[0]).toHaveClass(iconClass, "agent-task-header__status-icon");
      expect(icons[0]).toHaveAttribute("width", "12");
      expect(icons[0]).toHaveAttribute("aria-hidden", "true");
    }
    expect(status.querySelector(".agent-chat-chip__spinner") !== null).toBe(
      tone === "working",
    );
  },
);

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

it("keeps full chat and harness status discoverable when compacted", () => {
  const tab = {
    id: "chat:chrome",
    title: "A long chat title",
    kind: "chat" as const,
    position: 0,
    pinned: false,
    dirty: false,
  };
  useWorkbenchStore.setState({
    project: { id: "project:chrome", name: "Sage", roots: [] },
    tabs: [tab],
  });
  render(
    <AgentTaskHeader
      tab={tab}
      chatCount={1}
      snapshot={{
        chatId: tab.id,
        statusLabel: "Waiting for approval",
        statusTone: "waiting",
        latestText: "",
        streaming: false,
        waiting: true,
        working: false,
      }}
      sessionChrome={{
        chatId: tab.id,
        harnessStatus: "Disconnected; reconnect to continue",
        canCloseSession: false,
        canEndSession: false,
        busy: false,
        acpPage: null,
        messages: [],
        actions: {
          current: {
            closeSession: vi.fn(),
            endSession: vi.fn(),
            focusComposer: vi.fn(),
          },
        },
      }}
    />,
  );
  expect(
    screen.getByRole("status", { name: "Chat status: Waiting for approval" }),
  ).toHaveAttribute("title", "Waiting for approval");
  expect(screen.getByRole("status")).toHaveAttribute("tabindex", "0");
  expect(
    screen.getByLabelText(
      "Harness status: Disconnected; reconnect to continue",
    ),
  ).toHaveAttribute("title", "Disconnected; reconnect to continue");
  expect(
    screen.getByLabelText(
      "Harness status: Disconnected; reconnect to continue",
    ),
  ).toHaveAttribute("tabindex", "0");
});
