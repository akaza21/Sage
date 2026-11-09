import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import type { AgentSessionChrome } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { settleLazyModules } from "../test/settleLazyModules";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { AgentChatCenterHeader } from "./AgentChatCenterHeader";
import { preloadAgentHeaderMenu } from "./agentTaskHeaderLazy";

vi.mock("../features/chat/transcriptLazy", () => ({
  TranscriptPart: ({ chatId }: { chatId: string }) => (
    <span>Side tab {chatId}</span>
  ),
}));

const snapshot = (chatId: string, working: boolean): ChatLiveSnapshot => ({
  chatId,
  statusLabel: working ? "Working" : "Idle",
  statusTone: working ? "working" : "idle",
  latestText: "",
  streaming: false,
  waiting: false,
  working,
});

vi.mock("../features/chat/acp/useAcpChatLiveSnapshot", () => ({
  useAcpChatLiveSnapshot: (chatId: string) => snapshot(chatId, true),
}));

// The header menu body loads when the window first idles.
beforeAll(() => preloadAgentHeaderMenu());

const initialState = useWorkbenchStore.getState();
const parent: WorkbenchTab = {
  id: "chat:parent",
  kind: "chat",
  title: "Parent task",
  position: 0,
  pinned: false,
  dirty: false,
};
const side: WorkbenchTab = { ...parent, id: "side-1", title: "Side chat" };

function chrome(chatId: string, harnessStatus: string): AgentSessionChrome {
  return {
    chatId,
    harnessStatus,
    canCloseSession: true,
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
  };
}

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Sage", roots: [] },
    projects: [{ id: "project-a", name: "Sage", roots: [] }],
    tabs: [parent, side],
    activeTabId: side.id,
  } as never);
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

it("keeps the parent's own live session chrome while its side chat is focused", async () => {
  render(
    <AgentChatCenterHeader
      tabs={[parent, side]}
      paneIds={[parent.id, side.id]}
      focusedId={side.id}
      focusedSnapshot={snapshot(side.id, false)}
      chatCount={1}
      sessionChromeById={{
        [parent.id]: chrome(parent.id, "Parent live"),
        [side.id]: chrome(side.id, "Side live"),
      }}
      splitPercent={57}
    />,
  );
  await settleLazyModules();
  // Status and actions in the parent cell both belong to the parent.
  expect(screen.getByRole("status")).toHaveAccessibleName(
    "Chat status: Working",
  );
  expect(screen.getByLabelText("Harness status: Parent live")).toBeVisible();
  expect(screen.queryByLabelText("Harness status: Side live")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Chat actions for/ }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
  );
  expect(
    await screen.findByRole("menuitem", { name: "Close session" }),
  ).toBeInTheDocument();
  const more = screen.getByRole("button", { name: /Chat actions for/ });
  fireEvent.click(more);
  fireEvent.click(more);
  expect(screen.queryByRole("menuitem", { name: "Close session" })).toBeNull();
});

it("shows no session chrome the parent did not publish", async () => {
  render(
    <AgentChatCenterHeader
      tabs={[parent, side]}
      paneIds={[parent.id, side.id]}
      focusedId={side.id}
      focusedSnapshot={snapshot(side.id, false)}
      chatCount={1}
      sessionChromeById={{ [side.id]: chrome(side.id, "Side live") }}
      splitPercent={57}
    />,
  );
  await settleLazyModules();
  expect(screen.queryByLabelText(/^Harness status:/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /Chat actions for/ }));
  fireEvent.click(
    screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
  );
  expect(screen.queryByRole("menuitem", { name: "Close session" })).toBeNull();
});
