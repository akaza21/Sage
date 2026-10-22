import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { AgentTaskHeader } from "./AgentTaskHeader";

const initialState = useWorkbenchStore.getState();

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
  expect(
    screen.getByLabelText(
      "Harness status: Disconnected; reconnect to continue",
    ),
  ).toHaveAttribute("title", "Disconnected; reconnect to continue");
});
