import { createElement } from "react";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import {
  applyBrowseIntent,
  browseSessionForOwner,
  clearBrowseSessions,
  harnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../features/preview/harnessBrowse";
import { AgentChatPanes } from "./AgentChatPanes";
vi.mock("./surfaceRegistry", () => ({ renderRegisteredCanvas: () => null }));
vi.mock("../features/chat/ChatSurface", () => ({ ChatSurface: () => null }));
const initial = useWorkbenchStore.getState();
function owner(chatId: string): HarnessBrowseOwner {
  return {
    projectId: "project-a",
    projectGeneration: "1",
    transitionEpoch: 1,
    chatId,
    sessionId: `session-${chatId}`,
    runtimeEpoch: "7",
  } as HarnessBrowseOwner;
}
function seed(chatId: string) {
  const store = acpProjectionStoreFor(chatId);
  const scope = {
    projectId: "project-a",
    projectGeneration: "1",
    chatId,
  } as never;
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, {
    projectId: "project-a",
    chatId,
    title: chatId,
    projectionRevision: "1",
    currentSessionId: `session-${chatId}`,
    sessions: [{ id: `session-${chatId}`, runtimeEpoch: "7" }],
    turns: [],
    nextCursor: null,
  } as never);
}
function show() {
  clearAcpProjectionRegistry();
  clearBrowseSessions();
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectGeneration: 1,
    nativeTransitionEpoch: 1,
    projectTransitioning: false,
    tabs: ["chat-a", "chat-b", "chat-c"].map((id, position) => ({
      id,
      kind: "chat",
      title: id,
      position,
      pinned: false,
      dirty: false,
    })),
    activeTabId: "chat-b",
  });
  seed("chat-b");
  render(createElement(AgentChatPanes));
}
afterEach(() => {
  cleanup();
  clearBrowseSessions();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});
it("keeps a Chat's Preview across a switch to another Chat and back", () => {
  show();
  expect(
    applyBrowseIntent({
      owner: owner("chat-b"),
      url: "http://127.0.0.1:4173/",
    }),
  ).not.toBeNull();
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-a" }));
  expect(browseSessionForOwner(owner("chat-b"))).not.toBeNull();
  act(() => seed("chat-a"));
  expect(harnessBrowseOwner()?.chatId).toBe("chat-a");
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-b" }));
  expect(browseSessionForOwner(owner("chat-b"))?.href).toBe(
    "http://127.0.0.1:4173/",
  );
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-a" }));
  act(() => seed("chat-c"));
  act(() => useWorkbenchStore.setState({ activeTabId: "chat-c" }));
  expect(browseSessionForOwner(owner("chat-b"))).toBeNull();
});
it("releases every Preview when no Chat owns the centre", () => {
  show();
  applyBrowseIntent({ owner: owner("chat-b"), url: "http://127.0.0.1:4173/" });
  act(() => useWorkbenchStore.setState({ tabs: [], activeTabId: null }));
  expect(harnessBrowseOwner()).toBeNull();
  expect(browseSessionForOwner(owner("chat-b"))).toBeNull();
});
