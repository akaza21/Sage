import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../../../app/acpProjectionRegistry";
import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  AcpProjectionChangedEvent,
  AcpSessionConfigChangedV1,
} from "../../../domain/acp";
import { decodeAcpGoal } from "../../../domain/acpGoal";
import { acpDesktop } from "../../../services/acpDesktop";
import { acpHarnessDesktop } from "../../../services/acpHarnessDesktop";
import { useAcpChatProjection } from "./useAcpChatProjection";
import { page } from "./acpProjection.fixture";
import {
  harnessSignInStatus,
  refreshHarnessSignIn,
  resetHarnessSignInForTest,
  subscribeHarnessSignIn,
} from "../../harness/harnessSignIn";

const projection = {
  projectId: "project-1",
  chatId: "chat-1",
  title: "Durable chat",
  projectionRevision: "4",
  currentSessionId: null,
  sessions: [],
  turns: [],
  nextCursor: null,
} as unknown as AcpChatPage;

beforeEach(() => {
  useAcpProjectionStore.getState().clear();
  useWorkbenchStore.setState({
    project: { id: "project-1" } as never,
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-1",
    tabs: [{ id: "chat-1", kind: "chat" }] as never,
  });
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  resetHarnessSignInForTest();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
  clearAcpProjectionRegistry();
});

describe("useAcpChatProjection", () => {
  it("orders live and restored protocol refusals against the native account read", async () => {
    useWorkbenchStore.setState({
      harnesses: [
        {
          id: "account",
          name: "Account",
          description: "",
          source: "builtIn",
          installState: "available",
          transport: "acp",
          bindingId: "account-binding",
          accountModelDiscovery: true,
        },
      ],
      selectedHarnessId: "account",
      harnessAccount: "managedChatgpt",
      harnessAccountReadStartedAt: 100,
      harnessDiscoveryPhase: "ready",
    });
    const unsubscribe = subscribeHarnessSignIn(() => undefined);
    const running = page("running");
    running.sessions = running.sessions.map((session) => ({
      ...session,
      bindingId: "account-binding" as never,
    }));
    const failed = {
      ...page("failed", { kind: "authRequired" }),
      sessions: running.sessions,
      turns: page("failed", { kind: "authRequired" }).turns.map((turn) => ({
        ...turn,
        terminalAt: 200,
      })),
      projectionRevision: "13" as never,
    };
    let changed!: (event: AcpProjectionChangedEvent) => void;
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
      async (listener) => {
        changed = listener;
        return () => undefined;
      },
    );
    const load = vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(running);
    const hook = renderHook(() => useAcpChatProjection());
    await waitFor(() =>
      expect(hook.result.current?.turns[0].state).toBe("running"),
    );
    expect(harnessSignInStatus("account").state).toBe("signedIn");
    load.mockResolvedValue(failed);
    act(() =>
      changed({
        projectId: "project-1",
        chatId: "chat-1",
        sessionId: "session-1",
        projectionRevision: "13",
      } as AcpProjectionChangedEvent),
    );
    await waitFor(() =>
      expect(harnessSignInStatus("account").state).toBe("signedOut"),
    );
    hook.unmount();
    vi.spyOn(acpHarnessDesktop, "discoverCodex").mockImplementation(
      async (scope) =>
        ({
          ...scope,
          rootId: null,
          account: "apiKey",
          accountReadStartedAt: 300,
          models: [],
        }) as never,
    );
    await act(async () => {
      await refreshHarnessSignIn("account");
    });
    expect(harnessSignInStatus("account").state).toBe("signedIn");
    const restored = renderHook(() => useAcpChatProjection());
    await waitFor(() =>
      expect(restored.result.current?.turns[0].state).toBe("failed"),
    );
    expect(harnessSignInStatus("account").state).toBe("signedIn");
    restored.unmount();
    unsubscribe();
  });
  it.each([
    [100, false],
    [300, false],
    [100, true],
    [300, true],
  ] as const)(
    "orders a suspended refusal at 200 against a cached account read at %i (later turn: %s)",
    async (readStartedAt, laterTurn) => {
      useWorkbenchStore.setState({
        harnesses: [
          {
            id: "account",
            name: "Account",
            description: "",
            source: "builtIn",
            installState: "available",
            transport: "acp",
            bindingId: "account-binding",
            accountModelDiscovery: true,
          },
        ],
        selectedHarnessId: "account",
        harnessAccount: "managedChatgpt",
        harnessAccountReadStartedAt: readStartedAt,
        harnessDiscoveryPhase: "ready",
      });
      const unsubscribe = subscribeHarnessSignIn(() => undefined);
      const running = page("running");
      running.sessions = running.sessions.map((session) => ({
        ...session,
        bindingId: "account-binding" as never,
      }));
      const failed = {
        ...page("failed", { kind: "authRequired" }),
        sessions: [
          ...running.sessions,
          {
            ...running.sessions[0],
            id: "other-session" as never,
            bindingId: "other-binding" as never,
          },
        ],
        turns: [
          ...page("failed", { kind: "authRequired" }).turns.map((turn) => ({
            ...turn,
            terminalAt: 200,
          })),
          ...(laterTurn
            ? [
                {
                  ...running.turns[0],
                  id: "new-turn" as never,
                  sessionId: "other-session" as never,
                  ordinal: 2,
                },
              ]
            : []),
        ],
        currentSessionId: laterTurn
          ? ("other-session" as never)
          : running.currentSessionId,
        projectionRevision: "13" as never,
      };
      let changed!: (event: AcpProjectionChangedEvent) => void;
      vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
        async (listener) => {
          changed = listener;
          return () => undefined;
        },
      );
      const load = vi
        .spyOn(acpDesktop, "getChatPage")
        .mockResolvedValue(running);
      pinAcpProjectionIds(["chat-1"]);
      const hook = renderHook(() => useAcpChatProjection(true, "chat-1"));
      await waitFor(() =>
        expect(hook.result.current?.turns[0].state).toBe("running"),
      );
      let complete!: (value: AcpChatPage) => void;
      load.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            complete = resolve;
          }),
      );
      act(() =>
        changed({
          projectId: "project-1",
          chatId: "chat-1",
          sessionId: "session-1",
          projectionRevision: "13",
        } as AcpProjectionChangedEvent),
      );
      await waitFor(() => expect(complete).toBeDefined());
      hook.unmount();
      await act(async () => complete(failed));
      expect(harnessSignInStatus("account").state).toBe("signedIn");
      load.mockResolvedValue(failed);
      const restored = renderHook(() => useAcpChatProjection(true, "chat-1"));
      await waitFor(() =>
        expect(
          restored.result.current?.turns.find((turn) => turn.id === "turn-1")
            ?.state,
        ).toBe("failed"),
      );
      expect(harnessSignInStatus("account").state).toBe(
        readStartedAt < 200 ? "signedOut" : "signedIn",
      );
      restored.unmount();
      unsubscribe();
    },
  );
  it("subscribes config before bootstrap, coalesces reloads and recovers on renderer remount", async () => {
    let onConfig: ((event: AcpSessionConfigChangedV1) => void) | undefined;
    const dispose = vi.fn();
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
      () => undefined,
    );
    let resolveSubscription!: (dispose: () => void) => void;
    vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockImplementation(
      async (callback) => {
        onConfig = callback;
        return new Promise((resolve) => {
          resolveSubscription = resolve;
        });
      },
    );
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockResolvedValue(projection);
    const hook = renderHook(() => useAcpChatProjection());
    expect(load).not.toHaveBeenCalled();
    await act(async () => resolveSubscription(dispose));
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
    const event = {
      schemaVersion: 1 as const,
      sageProjectId: "project-1",
      sageChatId: "chat-1",
      sageAgentSessionId: "session-a",
      runtimeEpoch: "2",
      configRevision: "1",
    };
    act(() => onConfig?.(event));
    await new Promise((resolve) => setTimeout(resolve, 70));
    expect(load).toHaveBeenCalledTimes(1);
    hook.unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
    vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockResolvedValue(
      () => undefined,
    );
    renderHook(() => useAcpChatProjection());
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  });
  it("loads with native authority while retaining the disposable UI generation", async () => {
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockResolvedValue(projection);
    const { result } = renderHook(() => useAcpChatProjection());

    await waitFor(() => expect(result.current).toEqual(projection));
    expect(load).toHaveBeenCalledWith(
      {
        projectId: "project-1",
        projectGeneration: "3",
        chatId: "chat-1",
        cursor: null,
      },
      decodeAcpGoal,
    );
    expect(useAcpProjectionStore.getState().scope).toEqual({
      projectId: "project-1",
      projectGeneration: "7",
      chatId: "chat-1",
    });
  });

  it("reloads durable activity when returning to a Chat after background work", async () => {
    const chatB = {
      ...projection,
      chatId: "chat-2",
      title: "Second chat",
      projectionRevision: "2",
    } as AcpChatPage;
    const restoredA = {
      ...projection,
      projectionRevision: "6",
      turns: [
        {
          id: "turn-background",
          sessionId: "session-background",
          runtimeEpoch: "1",
          ordinal: 1,
          state: "completed",
          revision: "1",
          createdAt: 1,
          updatedAt: 2,
          terminalAt: 2,
          items: [],
          activities: [],
        },
      ],
    } as unknown as AcpChatPage;
    let chatALoads = 0;
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockImplementation(async (request) => {
        if (request.chatId === "chat-2") return chatB;
        chatALoads += 1;
        return chatALoads === 1 ? projection : restoredA;
      });
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
      () => undefined,
    );
    useWorkbenchStore.setState({
      tabs: [
        { id: "chat-1", kind: "chat" },
        { id: "chat-2", kind: "chat" },
      ] as never,
    });
    const { result } = renderHook(() => useAcpChatProjection());

    await waitFor(() => expect(result.current?.projectionRevision).toBe("4"));
    act(() => useWorkbenchStore.setState({ activeTabId: "chat-2" }));
    await waitFor(() => expect(result.current?.chatId).toBe("chat-2"));
    act(() => useWorkbenchStore.setState({ activeTabId: "chat-1" }));

    await waitFor(() => expect(load).toHaveBeenCalledTimes(3));
    await waitFor(() => expect(result.current?.projectionRevision).toBe("6"));
    expect(result.current?.turns[0]?.id).toBe("turn-background");
    expect(chatALoads).toBe(2);
  });

  it("clears and does not load while a project transition is blocking the UI", () => {
    useWorkbenchStore.setState({ projectTransitioning: true });
    const load = vi.spyOn(acpDesktop, "getChatPage");
    const { result } = renderHook(() => useAcpChatProjection());

    expect(result.current).toBeNull();
    expect(load).not.toHaveBeenCalled();
  });

  it("keeps a bound chat live when the active tab is a file", async () => {
    useWorkbenchStore.setState({
      activeTabId: "file:a",
      tabs: [
        { id: "file:a", kind: "file" },
        { id: "chat-1", kind: "chat" },
      ] as never,
    });
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockResolvedValue(projection);
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
      () => undefined,
    );
    const { result } = renderHook(() => useAcpChatProjection(true, "chat-1"));
    await waitFor(() => expect(result.current).toEqual(projection));
    expect(load).toHaveBeenCalledWith(
      {
        projectId: "project-1",
        projectGeneration: "3",
        chatId: "chat-1",
        cursor: null,
      },
      decodeAcpGoal,
    );
  });

  it("keeps two bound Chat pages from stealing each other's stream", async () => {
    const chatB = {
      ...projection,
      chatId: "chat-2",
      title: "Second chat",
      projectionRevision: "2",
    } as AcpChatPage;
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockImplementation(async (request) =>
        request.chatId === "chat-2" ? chatB : projection,
      );
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
      () => undefined,
    );
    useWorkbenchStore.setState({
      tabs: [
        { id: "chat-1", kind: "chat" },
        { id: "chat-2", kind: "chat" },
      ] as never,
      activeTabId: "chat-1",
    });
    const first = renderHook(() => useAcpChatProjection(true, "chat-1"));
    const second = renderHook(() => useAcpChatProjection(true, "chat-2"));
    await waitFor(() => expect(first.result.current?.chatId).toBe("chat-1"));
    await waitFor(() => expect(second.result.current?.chatId).toBe("chat-2"));
    expect(first.result.current?.projectionRevision).toBe("4");
    expect(second.result.current?.projectionRevision).toBe("2");
    expect(load).toHaveBeenCalledWith(
      {
        projectId: "project-1",
        projectGeneration: "3",
        chatId: "chat-1",
        cursor: null,
      },
      decodeAcpGoal,
    );
    expect(load).toHaveBeenCalledWith(
      {
        projectId: "project-1",
        projectGeneration: "3",
        chatId: "chat-2",
        cursor: null,
      },
      decodeAcpGoal,
    );
  });

  it("does not load or retain a listener for a non-ACP harness", () => {
    const load = vi.spyOn(acpDesktop, "getChatPage");
    const subscribe = vi.spyOn(acpDesktop, "subscribeProjectionChanges");
    const { result } = renderHook(() => useAcpChatProjection(false));

    expect(result.current).toBeNull();
    expect(load).not.toHaveBeenCalled();
    expect(subscribe).not.toHaveBeenCalled();
    expect(useAcpProjectionStore.getState().scope).toBeNull();
  });

  it("coalesces committed notices into one bounded authoritative refresh", async () => {
    vi.useFakeTimers();
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockResolvedValue(projection);
    type ProjectionNoticeHandler = (event: AcpProjectionChangedEvent) => void;
    let notify: ProjectionNoticeHandler | null = null;
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
      async (callback) => {
        notify = callback as typeof notify;
        return () => undefined;
      },
    );
    renderHook(() => useAcpChatProjection());
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));

    const send = notify as unknown as ProjectionNoticeHandler;
    send({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      projectionRevision: "5",
    } as AcpProjectionChangedEvent);
    send({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      projectionRevision: "6",
    } as AcpProjectionChangedEvent);
    await vi.advanceTimersByTimeAsync(50);
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  });

  it("subscribes before bootstrap so a transition-gap notice is recovered", async () => {
    vi.useFakeTimers();
    let resolveBootstrap!: (value: AcpChatPage) => void;
    const bootstrap = new Promise<AcpChatPage>((resolve) => {
      resolveBootstrap = resolve;
    });
    const refreshed = {
      ...projection,
      projectionRevision: "6",
    } as AcpChatPage;
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockReturnValueOnce(bootstrap)
      .mockResolvedValueOnce(refreshed);
    type ProjectionNoticeHandler = (event: AcpProjectionChangedEvent) => void;
    let notify: ProjectionNoticeHandler | null = null;
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
      async (callback) => {
        notify = callback;
        return () => undefined;
      },
    );
    renderHook(() => useAcpChatProjection());
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));

    const send = notify as unknown as ProjectionNoticeHandler;
    send({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      projectionRevision: "6",
    } as AcpProjectionChangedEvent);
    await vi.advanceTimersByTimeAsync(50);
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    resolveBootstrap(projection);
    await vi.waitFor(() =>
      expect(useAcpProjectionStore.getState().page?.projectionRevision).toBe(
        "6",
      ),
    );
  });

  it("recreates its disposable projection with one live native listener", async () => {
    const restored = {
      ...projection,
      projectionRevision: "5",
    } as AcpChatPage;
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockResolvedValueOnce(projection)
      .mockResolvedValueOnce(restored);
    const listeners: Array<(event: AcpProjectionChangedEvent) => void> = [];
    const disposers: Array<ReturnType<typeof vi.fn>> = [];
    const subscribe = vi
      .spyOn(acpDesktop, "subscribeProjectionChanges")
      .mockImplementation(async (callback) => {
        const dispose = vi.fn();
        listeners.push(callback);
        disposers.push(dispose);
        return dispose;
      });

    const firstRenderer = renderHook(() => useAcpChatProjection());
    await waitFor(() =>
      expect(firstRenderer.result.current?.projectionRevision).toBe("4"),
    );
    firstRenderer.rerender();
    expect(subscribe).toHaveBeenCalledTimes(1);

    firstRenderer.unmount();
    expect(disposers[0]).toHaveBeenCalledTimes(1);
    listeners[0]({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      projectionRevision: "99",
    } as AcpProjectionChangedEvent);
    expect(load).toHaveBeenCalledTimes(1);

    useAcpProjectionStore.getState().clear();
    const recreatedRenderer = renderHook(() => useAcpChatProjection());
    await waitFor(() =>
      expect(recreatedRenderer.result.current?.projectionRevision).toBe("5"),
    );
    expect(subscribe).toHaveBeenCalledTimes(2);
    expect(disposers[0]).toHaveBeenCalledTimes(1);
    expect(disposers[1]).not.toHaveBeenCalled();

    recreatedRenderer.unmount();
    expect(disposers[1]).toHaveBeenCalledTimes(1);
  });
});
