import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
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

function pageWithText(page: AcpChatPage, text: string): AcpChatPage {
  const [turn] = page.turns;
  return {
    ...page,
    turns: [
      {
        ...turn,
        items: [{ ...turn!.items[0], payload: { role: "assistant", text } }],
      },
    ],
  } as unknown as AcpChatPage;
}

function projectionBus(granted: Promise<void> = Promise.resolve()) {
  const listeners = new Set<(event: AcpProjectionChangedEvent) => void>();
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    async (callback) => {
      await granted;
      listeners.add(callback);
      return () => {
        listeners.delete(callback);
      };
    },
  );
  return {
    listeners: () => listeners.size,
    notify(event: { chatId: string; projectionRevision: string }) {
      for (const listener of [...listeners]) {
        listener({
          projectId: "project-1",
          sessionId: "session-1",
          ...event,
        } as AcpProjectionChangedEvent);
      }
    },
  };
}

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

  it("ignores other-chat notices and coalesces matching ones into the latest page", async () => {
    vi.useFakeTimers();
    let current = pageWithText(pageB, "Waiting in B");
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockImplementation(async () => current);
    const backend = projectionBus();
    const { result } = renderHook(() => useAcpChatLiveSnapshot("chat-2", true));
    await vi.waitFor(() =>
      expect(result.current.latestText).toBe("Waiting in B"),
    );

    current = pageWithText(pageB, "Changed while chat-1 spoke");
    backend.notify({ chatId: "chat-1", projectionRevision: "9" });
    await act(() => vi.advanceTimersByTimeAsync(50));
    expect(result.current.latestText).toBe("Waiting in B");

    current = pageWithText(pageB, "First reply in B");
    backend.notify({ chatId: "chat-2", projectionRevision: "3" });
    current = pageWithText(pageB, "Second reply in B");
    backend.notify({ chatId: "chat-2", projectionRevision: "4" });
    await act(() => vi.advanceTimersByTimeAsync(50));
    await vi.waitFor(() =>
      expect(result.current.latestText).toBe("Second reply in B"),
    );
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("keeps a page that arrives after the pane moved on out of the new chat", async () => {
    let finishLate!: (page: AcpChatPage) => void;
    let chat2Loads = 0;
    vi.spyOn(acpDesktop, "getChatPage").mockImplementation(async (request) => {
      if (request.chatId === "chat-1") return pageA;
      chat2Loads += 1;
      if (chat2Loads === 1) return pageB;
      return new Promise<AcpChatPage>((resolve) => {
        finishLate = resolve;
      });
    });
    const backend = projectionBus();
    const { result, rerender } = renderHook(
      ({ chatId }) => useAcpChatLiveSnapshot(chatId, true),
      { initialProps: { chatId: "chat-2" } },
    );
    await waitFor(() => expect(result.current.latestText).toBe("Waiting in B"));

    backend.notify({ chatId: "chat-2", projectionRevision: "3" });
    await waitFor(() => expect(finishLate).toBeTypeOf("function"));
    rerender({ chatId: "chat-1" });
    await waitFor(() =>
      expect(result.current.latestText).toBe("Streaming from A"),
    );
    expect(backend.listeners()).toBe(1);

    await act(async () => {
      finishLate(pageWithText(pageB, "Late reply for B"));
    });
    expect(result.current.chatId).toBe("chat-1");
    expect(result.current.latestText).toBe("Streaming from A");
  });

  it("releases its projection listener when the live pane unmounts", async () => {
    vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(pageB);
    const backend = projectionBus();
    const { result, unmount } = renderHook(() =>
      useAcpChatLiveSnapshot("chat-2", true),
    );
    await waitFor(() => expect(result.current.latestText).toBe("Waiting in B"));
    expect(backend.listeners()).toBe(1);

    unmount();
    expect(backend.listeners()).toBe(0);
  });

  it("releases a listener that is granted only after the pane unmounted", async () => {
    vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue(pageB);
    let grant!: () => void;
    const backend = projectionBus(
      new Promise<void>((resolve) => {
        grant = resolve;
      }),
    );
    const { unmount } = renderHook(() =>
      useAcpChatLiveSnapshot("chat-2", true),
    );
    unmount();

    await act(async () => {
      grant();
    });
    expect(backend.listeners()).toBe(0);
  });
});

it("returns one idle snapshot identity across renders while inactive", () => {
  const { result, rerender } = renderHook(
    ({ chatId }) => useAcpChatLiveSnapshot(chatId, false),
    { initialProps: { chatId: "chat-1" } },
  );
  const idle = result.current;
  rerender({ chatId: "chat-1" });
  expect(result.current).toBe(idle);
  rerender({ chatId: "chat-2" });
  expect(result.current).not.toBe(idle);
  expect(result.current.chatId).toBe("chat-2");
});
