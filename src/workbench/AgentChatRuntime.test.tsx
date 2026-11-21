import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  clearAcpProjectionRegistry,
  occupiedAcpProjectionSlotCount,
} from "../app/acpProjectionRegistry";
import {
  changeQueuedChatPrompt,
  persistQueuedChatPrompt,
} from "../app/chatAttachedContextState";
import { clearAgentChatOwner, pinAgentChatOwner } from "../app/agentChatOwner";
import { AgentSettingsSurface } from "../features/settings/AgentSettingsSurface";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { page as chatPage } from "../features/chat/acp/acpProjection.fixture";
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
  clearAgentChatOwner();
  useAgentSettingsState.getState().close();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
  useSessionAccessModeStore.setState(initialAccess, true);
  vi.restoreAllMocks();
});

it.each([false, true])(
  "receives Settings diagnostics without a queue (read already pending=%s)",
  async (delayed) => {
    const { page, listeners, configListeners } = nativeProjection();
    useWorkbenchStore.setState({
      chatUiById: { "chat-a": { ...queue(), queuedPrompts: [] } },
    });
    pinAgentChatOwner("chat-a");
    const tree = (settings: boolean) => (
      <>
        <AgentChatRuntime ids={["chat-a"]} visible={!settings} />
        {settings ? <AgentSettingsSurface chatRuntimeOwned /> : null}
      </>
    );
    const view = render(tree(false));
    await waitFor(() => expect(listeners.size).toBe(1));
    await act(async () => vi.dynamicImportSettled());
    let deliver!: (page: AcpChatPage) => void;
    vi.mocked(acpDesktop.getChatPage).mockImplementation(
      () =>
        new Promise((resolve) => {
          deliver = resolve;
        }),
    );
    const emit = () =>
      act(() => {
        listeners.forEach((listener) =>
          listener({
            projectId: "project-a",
            chatId: "chat-a",
            sessionId: "session-a",
            projectionRevision: "2",
          } as AcpProjectionChangedEvent),
        );
      });
    if (delayed) {
      emit();
      await waitFor(() => expect(deliver).toBeTypeOf("function"));
    }
    const pendingDelivery = delayed ? deliver : null;
    act(() => useAgentSettingsState.getState().open("Harnesses"));
    view.rerender(tree(true));
    await screen.findByRole("button", { name: "Refresh discovery" });
    if (!delayed) {
      emit();
      await waitFor(() => expect(deliver).toBeTypeOf("function"));
    }
    await act(async () =>
      (pendingDelivery ?? deliver)({
        ...page,
        projectionRevision: "2" as AcpChatPage["projectionRevision"],
        sessionNotices: [
          {
            level: "warning",
            diagnostic: true,
            text: "Unknown configuration key received in Settings",
            source: "fixture",
          },
        ],
      }),
    );
    expect(
      await screen.findByText("Unknown configuration key received in Settings"),
    ).toBeVisible();
    expect(listeners.size).toBe(1);
    expect(configListeners.size).toBe(1);
    // Other Settings pages and a minimized window do not retain a reader.
    act(() => useAgentSettingsState.getState().open("Chats"));
    await waitFor(() => expect(listeners.size).toBe(0));
    expect(configListeners.size).toBe(0);
    act(() => useAgentSettingsState.getState().open("Harnesses"));
    await waitFor(() => expect(listeners.size).toBe(1));
    act(() => {
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await waitFor(() => expect(listeners.size).toBe(0));
    expect(configListeners.size).toBe(0);
  },
);

it("shares the queued chat reader with Harnesses settings without losing a pending response or exceeding four native reads", async () => {
  const { listeners } = nativeProjection();
  const page: AcpChatPage = {
    ...chatPage("running"),
    projectId: "project-a" as AcpChatPage["projectId"],
    chatId: "chat-a" as AcpChatPage["chatId"],
    currentSessionId: "session-a" as AcpChatPage["currentSessionId"],
    sessions: [
      agentSessionFixture({
        state: "active",
        residency: "hot",
        runtimeEpoch: "2" as never,
      }),
    ],
  };
  page.turns[0].sessionId = page.currentSessionId!;
  useWorkbenchStore.setState({
    chatUiById: {
      "chat-a": {
        ...queue(),
        queuedPrompts: [],
        composerText: "Follow up after this turn",
      },
    },
  });
  // History and status reads also share the native four-read lease pool.
  let inFlight = 2;
  let peakReads = 2;
  let capacityErrors = 0;
  const pending: ((page: AcpChatPage) => void)[] = [];
  vi.mocked(acpDesktop.getChatPage)
    .mockResolvedValueOnce(page)
    .mockImplementation(() => {
      if (inFlight === 4) {
        capacityErrors += 1;
        return Promise.reject(new Error("Native read limit"));
      }
      inFlight += 1;
      peakReads = Math.max(peakReads, inFlight);
      return new Promise((resolve) =>
        pending.push((next) => {
          inFlight -= 1;
          resolve(next);
        }),
      );
    });
  pinAgentChatOwner("chat-a");
  useAgentSettingsState.getState().open("Chats");
  const tree = (settings: boolean) => (
    <>
      <AgentChatRuntime ids={["chat-a"]} visible={!settings} />
      {settings ? <AgentSettingsSurface chatRuntimeOwned /> : null}
    </>
  );
  const view = render(tree(false));
  await act(async () => vi.dynamicImportSettled());
  await act(async () => {
    expect(
      await persistQueuedChatPrompt(
        useWorkbenchStore,
        "chat-a",
        "binding-a",
        null,
      ),
    ).toBe(true);
    useWorkbenchStore.setState({
      chatUiById: {
        "chat-a": {
          ...queue(),
          queuedTurnBarrier: {
            afterOrder: { sessionOrdinal: 0, turnOrdinal: 1 },
            deadlineMs: Date.now() + 30_000,
          },
        },
      },
    });
  });
  const emit = (revision: string) =>
    act(() => {
      for (const listener of listeners)
        listener({
          projectId: "project-a",
          chatId: "chat-a",
          sessionId: "session-a",
          projectionRevision: revision,
        } as AcpProjectionChangedEvent);
    });
  emit("13");
  await waitFor(() => expect(pending).toHaveLength(1));
  view.rerender(tree(true));
  fireEvent.click(screen.getByRole("button", { name: "Harnesses" }));
  await screen.findByRole("button", { name: "Refresh discovery" });
  expect(listeners.size).toBe(1);
  const updated: AcpChatPage = {
    ...page,
    projectionRevision: "13" as AcpChatPage["projectionRevision"],
    sessionNotices: [
      {
        level: "warning",
        diagnostic: true,
        text: "Unknown configuration key from the pending read",
        source: "fixture",
      },
    ],
  };
  await act(async () => pending.shift()!(updated));
  expect(
    await screen.findByText("Unknown configuration key from the pending read"),
  ).toBeVisible();
  expect(queue().queuedPrompts?.map((item) => item.text)).toEqual([
    "Follow up after this turn",
  ]);
  expect(queue().queuedTurnBarrier).not.toBeNull();
  emit("14");
  await waitFor(() => expect(pending).toHaveLength(1));
  emit("15");
  await waitFor(() => expect(pending).toHaveLength(2));
  emit("16");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 75)));
  expect(peakReads).toBeLessThanOrEqual(4);
  expect(capacityErrors).toBe(0);
  // Closing Settings releases its subscription while the queued owner survives.
  view.rerender(tree(false));
  await act(async () => {
    pending.splice(0).forEach((resolve) => resolve(updated));
  });
  await waitFor(() => expect(pending).toHaveLength(1));
  await act(async () => pending.shift()!(updated));
  expect(peakReads).toBeLessThanOrEqual(4);
  expect(capacityErrors).toBe(0);
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
