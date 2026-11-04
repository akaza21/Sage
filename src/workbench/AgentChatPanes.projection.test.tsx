import { useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useStore } from "zustand";
import { afterEach, expect, it, vi } from "vitest";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import { clearAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { agentSessionFixture } from "../domain/acpProjection.fixture";
import type { AcpChatPage, AcpProjectionChangedEvent } from "../domain/acp";
import { acpDesktop } from "../services/acpDesktop";
import { useAcpChatProjection } from "../features/chat/acp/useAcpChatProjection";
import { AgentChatRuntimeHarness } from "../test/AgentChatRuntimeHarness";
import { AgentChatPanes } from "./AgentChatPanes";

vi.mock("./AgentChatCenterHeader", () => ({
  AgentChatCenterHeader: () => null,
}));
vi.mock("../features/chat/transcriptLazy", () => ({
  TranscriptPart: () => null,
}));
vi.mock("./surfaceRegistry", () => ({
  renderRegisteredCanvas: (_kind: string, tab: { id: string }) => (
    <ChatReader chatId={tab.id} />
  ),
}));

function ChatReader({ chatId }: { chatId: string }) {
  const page = useAcpChatProjection(true, chatId);
  const [draft, setDraft] = useState("");
  return (
    <>
      <p>Chat session: {page?.currentSessionId}</p>
      <input
        aria-label="Draft"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
      />
    </>
  );
}
function Artifacts() {
  const page = useStore(acpProjectionStoreFor("chat-1"), (state) => state.page);
  return (
    <aside aria-label="Agent artifacts">
      Artifact session: {page?.currentSessionId}
    </aside>
  );
}
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  clearAgentChatOwner();
  useWorkbenchStore.setState(initial, true);
  vi.restoreAllMocks();
});

it("updates visible artifacts from native revisions while Chat sleeps, then restores the live Chat and draft", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-1",
    tabs: [
      {
        id: "chat-1",
        kind: "chat",
        title: "Task",
        position: 0,
        pinned: false,
        dirty: false,
      },
    ],
  });
  const session = agentSessionFixture({
    id: "session-1" as AcpChatPage["currentSessionId"] & string,
    runtimeEpoch: "1" as NonNullable<
      AcpChatPage["sessions"][number]["runtimeEpoch"]
    >,
    residency: "hot",
  });
  let page: AcpChatPage = {
    projectId: "project-1" as AcpChatPage["projectId"],
    chatId: "chat-1" as AcpChatPage["chatId"],
    title: "Task",
    projectionRevision: "1" as AcpChatPage["projectionRevision"],
    currentSessionId: session.id,
    sessions: [session],
    turns: [],
    nextCursor: null,
  };
  const listeners = new Set<(event: AcpProjectionChangedEvent) => void>();
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    async (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  );
  vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockResolvedValue(
    () => undefined,
  );
  vi.spyOn(acpDesktop, "getChatPage").mockImplementation(async () => page);
  const tree = (visible: boolean) => (
    <AgentChatRuntimeHarness>
      <AgentChatPanes visible={visible} />
      <Artifacts />
    </AgentChatRuntimeHarness>
  );
  const view = render(tree(true));
  const artifacts = screen.getByRole("complementary", {
    name: "Agent artifacts",
  });
  await waitFor(() =>
    expect(artifacts).toHaveTextContent("Artifact session: session-1"),
  );
  const draft = screen.getByRole("textbox", { name: "Draft" });
  fireEvent.change(draft, { target: { value: "Keep this draft" } });
  view.rerender(tree(false));
  expect(draft).not.toBeVisible();
  const nextSession = agentSessionFixture({
    ...session,
    id: "session-2" as typeof session.id,
    runtimeEpoch: "2" as typeof session.runtimeEpoch,
  });
  page = {
    ...page,
    projectionRevision: "2" as AcpChatPage["projectionRevision"],
    currentSessionId: nextSession.id,
    sessions: [nextSession],
  };
  act(() => {
    for (const listener of listeners)
      listener({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-2",
        projectionRevision: "2",
      } as AcpProjectionChangedEvent);
  });
  await waitFor(() =>
    expect(artifacts).toHaveTextContent("Artifact session: session-2"),
  );
  expect(draft).not.toBeVisible();
  view.rerender(tree(true));
  await screen.findByText("Chat session: session-2");
  expect(screen.getByRole("textbox", { name: "Draft" })).toBe(draft);
  expect(draft).toHaveValue("Keep this draft");
});
