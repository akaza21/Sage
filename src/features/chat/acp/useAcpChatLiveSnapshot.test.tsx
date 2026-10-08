import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  AcpProjectionChangedEvent,
} from "../../../domain/acp";
import { decodeAcpGoal } from "../../../domain/acpGoal";
import { acpDesktop } from "../../../services/acpDesktop";
import { useAcpChatLiveSnapshot } from "./useAcpChatLiveSnapshot";

const pageA = {
  projectId: "project-1",
  chatId: "chat-1",
  title: "A",
  projectionRevision: "4",
  currentSessionId: "session-1",
  sessions: [],
  turns: [
    {
      id: "turn-a",
      sessionId: "session-1",
      runtimeEpoch: "1",
      ordinal: 1,
      state: "running",
      revision: "1",
      createdAt: 1,
      updatedAt: 1,
      terminalAt: null,
      items: [
        {
          id: "item-a",
          kind: "text",
          ordinal: 1,
          origin: "primary",
          lifecycle: "streaming",
          payload: { role: "assistant", text: "Streaming from A" },
        },
      ],
      activities: [],
    },
  ],
  nextCursor: null,
} as unknown as AcpChatPage;

const pageB = {
  ...pageA,
  chatId: "chat-2",
  projectionRevision: "2",
  turns: [
    {
      ...pageA.turns[0],
      id: "turn-b",
      state: "waitingUser",
      items: [
        {
          ...pageA.turns[0]?.items[0],
          id: "item-b",
          payload: { role: "assistant", text: "Waiting in B" },
        },
      ],
    },
  ],
} as unknown as AcpChatPage;

beforeEach(() => {
  useWorkbenchStore.setState({
    project: { id: "project-1" } as never,
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-1",
    tabs: [
      { id: "chat-1", kind: "chat" },
      { id: "chat-2", kind: "chat" },
    ] as never,
  });
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  vi.restoreAllMocks();
});

describe("useAcpChatLiveSnapshot", () => {
  it("keeps an unfocused chat live without using the focused projection store", async () => {
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockImplementation(async (request) =>
        request.chatId === "chat-2" ? pageB : pageA,
      );
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
      () => undefined,
    );
    const { result } = renderHook(() => useAcpChatLiveSnapshot("chat-2", true));

    await waitFor(() =>
      expect(result.current.statusLabel).toBe("Waiting for user"),
    );
    expect(result.current.latestText).toBe("Waiting in B");
    expect(result.current.waiting).toBe(true);
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

  it("ignores other-chat notices and coalesces matching ones", async () => {
    vi.useFakeTimers();
    const load = vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(pageB);
    type Handler = (event: AcpProjectionChangedEvent) => void;
    let notify: Handler | null = null;
    vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
      async (callback) => {
        notify = callback;
        return () => undefined;
      },
    );
    renderHook(() => useAcpChatLiveSnapshot("chat-2", true));
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));

    const send = notify as unknown as Handler;
    send({
      projectId: "project-1",
      chatId: "chat-1",
      sessionId: "session-1",
      projectionRevision: "9",
    } as AcpProjectionChangedEvent);
    send({
      projectId: "project-1",
      chatId: "chat-2",
      sessionId: "session-1",
      projectionRevision: "3",
    } as AcpProjectionChangedEvent);
    send({
      projectId: "project-1",
      chatId: "chat-2",
      sessionId: "session-1",
      projectionRevision: "4",
    } as AcpProjectionChangedEvent);
    await vi.advanceTimersByTimeAsync(50);
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  });

  it("drops its listener when the live pane unmounts", async () => {
    const dispose = vi.fn();
    vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(pageB);
    const subscribe = vi
      .spyOn(acpDesktop, "subscribeProjectionChanges")
      .mockResolvedValue(dispose);
    const { unmount } = renderHook(() =>
      useAcpChatLiveSnapshot("chat-2", true),
    );
    await waitFor(() => expect(subscribe).toHaveBeenCalledTimes(1));
    unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
  });
});
