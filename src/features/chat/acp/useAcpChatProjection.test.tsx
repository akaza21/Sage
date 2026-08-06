import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  AcpProjectionChangedEvent,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { useAcpChatProjection } from "./useAcpChatProjection";

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
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
});

describe("useAcpChatProjection", () => {
  it("loads with native authority while retaining the disposable UI generation", async () => {
    const load = vi
      .spyOn(acpDesktop, "getChatPage")
      .mockResolvedValue(projection);
    const { result } = renderHook(() => useAcpChatProjection());

    await waitFor(() => expect(result.current).toEqual(projection));
    expect(load).toHaveBeenCalledWith({
      projectId: "project-1",
      projectGeneration: "3",
      chatId: "chat-1",
      cursor: null,
    });
    expect(useAcpProjectionStore.getState().scope).toEqual({
      projectId: "project-1",
      projectGeneration: "7",
      chatId: "chat-1",
    });
  });

  it("clears and does not load while a project transition is blocking the UI", () => {
    useWorkbenchStore.setState({ projectTransitioning: true });
    const load = vi.spyOn(acpDesktop, "getChatPage");
    const { result } = renderHook(() => useAcpChatProjection());

    expect(result.current).toBeNull();
    expect(load).not.toHaveBeenCalled();
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

  it("recreates from native authority with exactly one live projection listener", async () => {
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
