import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { normalizeProjectionPage } from "../app/acpProjectionMerge";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, AcpChatStatus } from "../domain/acp";
import { page } from "../features/chat/acp/acpProjection.fixture";
import { createBrowserChatTab } from "../services/browserChat";
import {
  resetAgentRailInteractions,
  useAgentRailInteractions,
} from "./agentRailInteractions";
import { railPill, railTrailing } from "./railAttentionLabels";
import {
  agentRailTrailingState,
  compareAgentRailViewChats,
} from "./agentRailStatusView";
import {
  mirrorAgentRailStatus,
  railStatusForTurn,
} from "./useAgentRailProjection";

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

it("uses the newest turn for approvals regardless of native page order", () => {
  seed();
  const { result } = renderHook(() => useAgentRailInteractions());
  const current = page("waitingApproval");
  const older = {
    ...current.turns[0],
    id: "older" as never,
    ordinal: 0,
    state: "completed" as const,
  };
  const newer = { ...current.turns[0], ordinal: 1 };
  for (const turns of [
    [newer, older],
    [older, newer],
  ]) {
    act(() => mirrorAgentRailProjection({ ...current, turns }));
    expect(useWorkbenchStore.getState().tabs[0].status).toBe("waiting");
    expect(result.current.attention.get("chat-1")).toBe("approval");
  }
});

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
  expect(result.current.attention.get("chat-1")).toBe("approval");
  act(() => mirrorAgentRailProjection(page("completed")));
  expect(result.current.attention.has("chat-1")).toBe(false);
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

it("refreshes Recents for a coalesced completed turn and keeps unchanged-turn activity stable", () => {
  seed();
  mirrorAgentRailProjection(page("completed"));
  const previous = useWorkbenchStore.getState().tabs[0];
  const other = { ...previous, id: "chat-2", updatedAt: 10 };
  useWorkbenchStore.setState({ tabs: [previous, other] });
  const next = page("completed");
  next.turns = [
    {
      ...next.turns[0],
      id: "turn-2" as (typeof next.turns)[0]["id"],
      createdAt: 20,
      updatedAt: 30,
      terminalAt: 30,
    },
  ];
  mirrorAgentRailProjection(next);
  const tabs = useWorkbenchStore.getState().tabs;
  expect(tabs[0]).toMatchObject({
    title: previous.title,
    status: "complete",
    updatedAt: 30,
  });
  expect(
    [...tabs]
      .sort((left, right) => compareAgentRailViewChats(left, right, "recency"))
      .map((tab) => tab.id),
  ).toEqual(["chat-1", "chat-2"]);
  next.turns = [{ ...next.turns[0], updatedAt: 999 }];
  mirrorAgentRailProjection(next);
  expect(useWorkbenchStore.getState().tabs).toBe(tabs);
});

it("creates honestly untitled and unpinned browser chats", () => {
  expect(createBrowserChatTab([], 12)).toMatchObject({
    title: "Chat: Chat",
    pinned: false,
  });
});

function mirrorAgentRailProjection(page: AcpChatPage) {
  mirrorAgentRailStatus({
    ...page,
    latestTurn: normalizeProjectionPage(page).turns.at(-1) ?? null,
  });
}

const settled = {
  state: "completed" as const,
  updatedAt: 5,
  createdAt: 2,
  terminalAt: 5,
};
function mirrorLatest(latestTurn: AcpChatStatus["latestTurn"]) {
  act(() =>
    mirrorAgentRailStatus({
      projectId: "project-1" as never,
      chatId: "chat-1" as never,
      title: "Chat",
      latestTurn,
    }),
  );
  return useWorkbenchStore.getState().tabs[0];
}

it("keeps a settled turn working while its background work runs", () => {
  seed();
  const running = mirrorLatest({ ...settled, background: true });
  expect(running.status).toBe("working");
  // Not Done or Unread until the background work ends.
  expect(running.finishedAt).toBeUndefined();
  expect(mirrorLatest(settled)).toMatchObject({
    status: "complete",
    finishedAt: 5,
  });
});

it("labels a sign-in failure Sign in, not Failed", () => {
  seed();
  const { result } = renderHook(() => useAgentRailInteractions());
  const tab = mirrorLatest({ ...settled, state: "failed", signIn: true });
  const attention = result.current.attention.get("chat-1");
  expect(
    railTrailing(agentRailTrailingState({ status: tab.status, unread: false }), attention),
  ).toEqual({ kind: "failed", label: "Sign in" });
  mirrorLatest({ ...settled, state: "failed" });
  expect(result.current.attention.get("chat-1")).toBeUndefined();
});

it("keeps a question's Needs input pill without a mounted chat", () => {
  seed();
  const { result } = renderHook(() => useAgentRailInteractions());
  mirrorLatest({ ...settled, state: "waitingUser", terminalAt: null });
  expect(railPill(false, result.current.attention.get("chat-1"))).toBe(
    "Needs input",
  );
});
