import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../app/acpProjectionRegistry";
import { emptyChatOwnershipEntry } from "../../app/chatOwnershipProjection";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage, AcpProjectionChangedEvent } from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import AgentSettingsExecution from "./AgentSettingsExecution";

const initial = useWorkbenchStore.getState();
const initialAccess = useSessionAccessModeStore.getState();

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
  useSessionAccessModeStore.setState(initialAccess, true);
  vi.restoreAllMocks();
});

function page(
  chatId: string,
  state: "running" | "completed",
  revision: string,
): AcpChatPage {
  return {
    projectId: "project-a",
    chatId,
    title: chatId,
    projectionRevision: revision,
    currentSessionId: "session-a",
    sessions: [
      agentSessionFixture({
        state: "active",
        residency: "hot",
        runtimeEpoch: "1" as never,
      }),
    ],
    turns: [
      {
        id: "turn-a",
        sessionId: "session-a",
        runtimeEpoch: "1",
        ordinal: 0,
        state,
        revision,
        createdAt: 1,
        updatedAt: 2,
        terminalAt: state === "completed" ? 2 : null,
        items: [],
        activities: [],
        browserOperations: [],
        browserOperationsCollapsed: 0,
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

it("keeps idle pinned chats asleep without installing native listeners or fetching pages", () => {
  const subscribe = vi.spyOn(acpDesktop, "subscribeProjectionChanges");
  const load = vi.spyOn(acpDesktop, "getChatPage");
  pinAcpProjectionIds(["chat-a", "chat-b"]);
  useWorkbenchStore.setState({
    tabs: ["chat-a", "chat-b"].map((id, position) => ({
      id,
      kind: "chat",
      title: id,
      position,
      dirty: false,
      pinned: false,
    })),
    chatUiById: {},
  });
  const view = render(<AgentSettingsExecution />);
  expect(subscribe).not.toHaveBeenCalled();
  expect(load).not.toHaveBeenCalled();
  expect(view.container).toBeEmptyDOMElement();
});

it("continues both pinned queues from native wake events without mounting heavy chat UI or a third runtime", async () => {
  const ids = ["chat-a", "chat-b", "chat-c"];
  const pages = new Map(ids.map((id) => [id, page(id, "running", "1")]));
  const listeners = new Set<(event: AcpProjectionChangedEvent) => void>();
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    async (callback) => {
      listeners.add(callback);
      return () => {
        listeners.delete(callback);
      };
    },
  );
  vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockResolvedValue(
    () => undefined,
  );
  vi.spyOn(acpDesktop, "getChatPage").mockImplementation(async (request) =>
    pages.get(request.chatId)!,
  );
  vi.spyOn(
    getWorkbenchPorts().project,
    "replaceChatQueuedPrompts",
  ).mockResolvedValue(undefined);
  const send = vi
    .spyOn(acpDesktop, "sendTurn")
    .mockImplementation(async (request) => {
      const current = pages.get(request.chatId)!;
      const next = {
        ...current,
        projectionRevision: "3",
        turns: [
          ...current.turns,
          {
            ...current.turns[0],
            id: "next",
            ordinal: 1,
            state: "running",
            terminalAt: null,
          },
        ],
      } as unknown as AcpChatPage;
      pages.set(request.chatId, next);
      return { projection: next };
    });
  useSessionAccessModeStore.setState({ ready: true, error: null, modes: {} });
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectGeneration: 1,
    nativeTransitionEpoch: 7,
    projectTransitioning: false,
    chatOwnershipEpoch: 1,
    activeTabId: "chat-b",
    tabs: ids.map((id, position) => ({
      id,
      kind: "chat",
      title: id,
      position,
      dirty: false,
      pinned: false,
    })),
    harnesses: [
      {
        id: "adapter-a",
        name: "Adapter",
        description: "ACP",
        transport: "acp",
        source: "builtIn",
        installState: "available",
        bindingId: "binding-a",
        accountModelDiscovery: false,
        accessModes: [
          {
            id: "readOnly",
            label: "Read only",
            description: "Read",
            fallback: true,
          },
        ],
      },
    ],
    selectedHarnessId: "adapter-a",
    selectedModelId: null,
    harnessDiscoveryPhase: "ready",
    chatUiById: Object.fromEntries(
      ids.map((id) => [
        id,
        {
          ...emptyChatOwnershipEntry(),
          harnessId: "adapter-a",
          queuedPrompts: [
            {
              id: `queued-${id}`,
              text: `Continue ${id}`,
              attachedContext: [],
              bindingId: "binding-a",
              modelId: null,
              status: "queued",
            },
          ],
        },
      ]),
    ),
  });
  pinAcpProjectionIds(ids);
  const view = render(<AgentSettingsExecution />);
  await waitFor(() =>
    expect(
      acpProjectionStoreFor("chat-b").getState().page?.turns[0].state,
    ).toBe("running"),
  );
  expect(listeners.size).toBe(2);
  expect(send).not.toHaveBeenCalled();
  act(() => {
    for (const id of ids.slice(0, 2)) {
      pages.set(id, page(id, "completed", "2"));
      const event = {
        projectId: "project-a",
        chatId: id,
        sessionId: "session-a",
        projectionRevision: "2",
      } as AcpProjectionChangedEvent;
      listeners.forEach((listener) => listener(event));
    }
  });
  await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
  for (const chatId of ids.slice(0, 2)) {
    expect(send).toHaveBeenCalledWith(
      expect.objectContaining({
        chatId,
        projectGeneration: "7",
        prompt: `Continue ${chatId}`,
      }),
    );
    await waitFor(() =>
      expect(
        useWorkbenchStore.getState().chatUiById[chatId].queuedPrompts,
      ).toHaveLength(0),
    );
  }
  expect(
    useWorkbenchStore.getState().chatUiById["chat-c"].queuedPrompts,
  ).toHaveLength(1);
  expect(view.container).toBeEmptyDOMElement();
  view.unmount();
  expect(listeners.size).toBe(0);
});
