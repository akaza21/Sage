import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  clearAcpProjectionRegistry,
  occupiedAcpProjectionSlotCount,
} from "../app/acpProjectionRegistry";
import { changeQueuedChatPrompt } from "../app/chatAttachedContextState";
import { emptyChatOwnershipEntry } from "../app/chatOwnershipProjection";
import { useSessionAccessModeStore } from "../app/sessionAccessModeStore";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, AcpProjectionChangedEvent } from "../domain/acp";
import { agentSessionFixture } from "../domain/acpProjection.fixture";
import type { QueuedChatPrompt } from "../domain/chatOwnership";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { acpDesktop } from "../services/acpDesktop";
import AgentChatRuntime from "./AgentChatRuntime";

const initial = useWorkbenchStore.getState();
const initialAccess = useSessionAccessModeStore.getState();
const prompt: QueuedChatPrompt = {
  id: "queued-a",
  text: "Continue the task",
  attachedContext: [],
  bindingId: "binding-a",
  modelId: null,
  status: "queued",
};
const queue = () => useWorkbenchStore.getState().chatUiById["chat-a"];

beforeEach(() => {
  useSessionAccessModeStore.setState({ ready: true, error: null, modes: {} });
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    projectGeneration: 1,
    nativeTransitionEpoch: 7,
    projectTransitioning: false,
    chatOwnershipEpoch: 1,
    activeTabId: "chat-a",
    tabs: [
      {
        id: "chat-a",
        kind: "chat",
        title: "Task",
        position: 0,
        dirty: false,
        pinned: false,
      },
    ],
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
    chatUiById: {
      "chat-a": {
        ...emptyChatOwnershipEntry(),
        harnessId: "adapter-a",
        queuedPrompts: [prompt],
      },
    },
  });
});

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
  useSessionAccessModeStore.setState(initialAccess, true);
  vi.restoreAllMocks();
});

function nativeProjection() {
  const listeners = new Set<(event: AcpProjectionChangedEvent) => void>();
  const configListeners = new Set<unknown>();
  const page = {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Task",
    projectionRevision: "1",
    currentSessionId: "session-a",
    sessions: [
      agentSessionFixture({
        state: "active",
        residency: "hot",
        runtimeEpoch: "1" as never,
      }),
    ],
    turns: [],
    nextCursor: null,
  } as unknown as AcpChatPage;
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    async (listener) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  );
  vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockImplementation(
    async (listener) => {
      configListeners.add(listener);
      return () => void configListeners.delete(listener);
    },
  );
  vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(page);
  vi.spyOn(
    getWorkbenchPorts().project,
    "replaceChatQueuedPrompts",
  ).mockResolvedValue(undefined);
  return { page, listeners, configListeners };
}

it("sleeps after a hidden submission blocks and wakes on explicit retry", async () => {
  const { page, listeners, configListeners } = nativeProjection();
  let failSubmission!: (reason: Error) => void;
  const firstSubmission = new Promise<never>((_, reject) => {
    failSubmission = reject;
  });
  vi.spyOn(acpDesktop, "sendTurn")
    .mockImplementationOnce(() => firstSubmission)
    .mockResolvedValue({ projection: page });
  const view = render(<AgentChatRuntime ids={["chat-a"]} visible />);
  await waitFor(() =>
    expect(queue().queuedPrompts?.[0].status).toBe("sending"),
  );

  view.rerender(<AgentChatRuntime ids={["chat-a"]} visible={false} />);
  expect(listeners.size).toBe(1);
  expect(configListeners.size).toBe(1);
  await act(async () => failSubmission(new Error("Harness unavailable")));
  await waitFor(() =>
    expect(queue().queuedPrompts?.[0].status).toBe("blocked"),
  );
  expect(listeners.size).toBe(0);
  expect(configListeners.size).toBe(0);
  expect(occupiedAcpProjectionSlotCount()).toBe(0);

  act(() => {
    changeQueuedChatPrompt(useWorkbenchStore, "chat-a", prompt.id, "queued");
  });
  await waitFor(() => expect(queue().queuedPrompts).toEqual([]));
  // The accepted submission must keep listening for the new turn even with
  // an empty queue. Once the turn-start deadline expires it may sleep again.
  expect(queue().queuedTurnBarrier).not.toBeNull();
  expect(listeners.size).toBe(1);
  expect(configListeners.size).toBe(1);
  act(() => {
    useWorkbenchStore.setState({
      chatUiById: {
        "chat-a": {
          ...queue(),
          queuedTurnBarrier: { afterOrder: null, deadlineMs: Date.now() - 1 },
        },
      },
    });
  });
  await waitFor(() => expect(queue().queuedTurnBarrier).toBeNull());
  expect(listeners.size).toBe(0);
  expect(configListeners.size).toBe(0);
  expect(occupiedAcpProjectionSlotCount()).toBe(0);
});

it.each(["blocked", "outcomeUnknown"] as const)(
  "keeps a hidden FIFO queue asleep behind a %s head",
  async (status) => {
    const { listeners, configListeners } = nativeProjection();
    useWorkbenchStore.setState({
      chatUiById: {
        "chat-a": {
          ...queue(),
          queuedPrompts: [
            { ...prompt, status },
            { ...prompt, id: "queued-b" },
          ],
        },
      },
    });
    render(<AgentChatRuntime ids={["chat-a"]} visible={false} />);
    await act(async () => vi.dynamicImportSettled());
    expect(listeners.size).toBe(0);
    expect(configListeners.size).toBe(0);
    expect(occupiedAcpProjectionSlotCount()).toBe(0);
    expect(queue().queuedPrompts?.map((item) => item.status)).toEqual([
      status,
      "queued",
    ]);
  },
);
