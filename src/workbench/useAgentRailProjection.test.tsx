import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { page } from "../features/chat/acp/acpProjection.fixture";
import { createBrowserChatTab } from "../services/browserChat";
import {
  mirrorAgentRailProjection,
  railStatusForTurn,
} from "./useAgentRailProjection";
import {
  resetAgentRailInteractions,
  useAgentRailInteractions,
} from "./agentRailInteractions";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  resetAgentRailInteractions();
  useWorkbenchStore.setState(initial, true);
});
function seed() {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "A", roots: [] },
    tabs: [
      {
        id: "chat-1",
        kind: "chat",
        title: "Chat: Chat",
        pinned: false,
        dirty: false,
        position: 0,
        updatedAt: 1,
      },
    ],
  });
}

it("mirrors real lifecycle status, native first-prompt titles, and approval attention", () => {
  seed();
  const { result } = renderHook(() => useAgentRailInteractions());
  act(() => mirrorAgentRailProjection(page("running")));
  expect(useWorkbenchStore.getState().tabs[0]).toMatchObject({
    title: "Durable chat",
    status: "working",
    updatedAt: 2,
  });
  act(() => mirrorAgentRailProjection(page("waitingApproval")));
  expect(result.current.approval.has("chat-1")).toBe(true);
  act(() => mirrorAgentRailProjection(page("completed")));
  expect(result.current.approval.has("chat-1")).toBe(false);
  expect(useWorkbenchStore.getState().tabs[0].status).toBe("complete");
  expect(railStatusForTurn("waitingUser")).toBe("waiting");
  expect(railStatusForTurn("deliveryUnknown")).toBe("failed");
  expect(railStatusForTurn("cancelled")).toBe("idle");
});

it("does not update the rail or recency per streamed token, and rejects foreign projects", () => {
  seed();
  mirrorAgentRailProjection(page("running"));
  const tabs = useWorkbenchStore.getState().tabs;
  const next = page("running");
  next.turns = [{ ...next.turns[0], updatedAt: 999 }];
  mirrorAgentRailProjection(next);
  expect(useWorkbenchStore.getState().tabs).toBe(tabs);
  const foreign = {
    ...page("completed"),
    projectId: "foreign" as typeof next.projectId,
  };
  mirrorAgentRailProjection(foreign);
  expect(useWorkbenchStore.getState().tabs).toBe(tabs);
});

it("creates honestly untitled and unpinned browser chats", () => {
  expect(createBrowserChatTab([], 12)).toMatchObject({
    title: "Chat: Chat",
    pinned: false,
  });
});
