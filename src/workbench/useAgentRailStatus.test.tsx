import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import type { AcpProjectionChangedEvent } from "../domain/acp";
import type { AcpChatStatus } from "../domain/acp";
import { page } from "../features/chat/acp/acpProjection.fixture";
import { acpDesktop } from "../services/acpDesktop";
import {
  resetAgentRailInteractions,
  useAgentRailInteractions,
} from "./agentRailInteractions";
import { useAgentRailStatus } from "./useAgentRailStatus";

vi.mock("../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetAgentRailInteractions();
  useWorkbenchStore.setState(initial, true);
});

function status(state: Parameters<typeof page>[0]): AcpChatStatus {
  const current = page(state);
  return { ...current, latestTurn: current.turns[0] };
}

function seed() {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-2",
    tabs: ["chat-1", "chat-2"].map((id, position) => ({
      id,
      position,
      kind: "chat" as const,
      title: "Chat",
      pinned: false,
      dirty: false,
    })),
  });
}

it("updates an unmounted background chat on completion and approval without activating it", async () => {
  seed();
  let notify!: (event: AcpProjectionChangedEvent) => void;
  const dispose = vi.fn();
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    async (callback) => {
      notify = callback;
      return dispose;
    },
  );
  let current = status("running");
  const load = vi
    .spyOn(acpDesktop, "getChatStatus")
    .mockImplementation(async (request) => ({
      ...current,
      chatId: request.chatId,
    }));
  const hook = renderHook(() => {
    useAgentRailStatus(true);
    return useAgentRailInteractions();
  });
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0].status).toBe("working"),
  );
  expect(load).toHaveBeenCalledWith(
    expect.objectContaining({ projectGeneration: "3", chatId: "chat-1" }),
  );
  const event = {
    projectId: current.projectId,
    chatId: current.chatId,
    sessionId: "session-1" as never,
    projectionRevision: "2" as never,
  };
  current = status("waitingApproval");
  act(() => {
    for (let index = 0; index < 100; index += 1) notify(event);
  });
  await waitFor(() =>
    expect(hook.result.current.approval.has("chat-1")).toBe(true),
  );
  expect(load).toHaveBeenCalledTimes(3);
  current = status("completed");
  act(() => notify(event));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0].status).toBe("complete"),
  );
  expect(hook.result.current.approval.has("chat-1")).toBe(false);
  expect(useWorkbenchStore.getState().activeTabId).toBe("chat-2");
  hook.unmount();
  expect(dispose).toHaveBeenCalledOnce();
});

it("discards in-flight status reads across project generations and unsubscribes", async () => {
  seed();
  const dispose = vi.fn();
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(dispose);
  let resolve!: (value: AcpChatStatus) => void;
  vi.spyOn(acpDesktop, "getChatStatus").mockImplementation(
    () =>
      new Promise((complete) => {
        resolve = complete;
      }),
  );
  const hook = renderHook(() => useAgentRailStatus(true));
  await waitFor(() => expect(resolve).toBeDefined());
  act(() =>
    useWorkbenchStore.setState({
      projectGeneration: 8,
      projectTransitioning: true,
    }),
  );
  await act(async () => resolve(status("waitingApproval")));
  expect(useWorkbenchStore.getState().tabs[0].status).toBeUndefined();
  expect(dispose).toHaveBeenCalledOnce();
  hook.unmount();
});

it("retains changes arriving during a read and ignores foreign or absent chats", async () => {
  seed();
  useWorkbenchStore.setState({
    tabs: useWorkbenchStore.getState().tabs.slice(0, 1),
  });
  let notify!: (event: AcpProjectionChangedEvent) => void;
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    async (callback) => {
      notify = callback;
      return () => undefined;
    },
  );
  let complete!: (value: AcpChatStatus) => void;
  const load = vi
    .spyOn(acpDesktop, "getChatStatus")
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    )
    .mockResolvedValue(status("completed"));
  renderHook(() => useAgentRailStatus(true));
  await waitFor(() => expect(complete).toBeDefined());
  const event = {
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    projectionRevision: "2",
  } as AcpProjectionChangedEvent;
  act(() => {
    notify({ ...event, projectId: "foreign" as never });
    notify({ ...event, chatId: "absent" as never });
    notify(event);
  });
  await act(async () => complete(status("running")));
  await waitFor(() =>
    expect(useWorkbenchStore.getState().tabs[0].status).toBe("complete"),
  );
  expect(load).toHaveBeenCalledTimes(2);
});

it("disposes a late event subscription without bootstrapping after unmount", async () => {
  seed();
  let complete!: (dispose: () => void) => void;
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockImplementation(
    () =>
      new Promise((resolve) => {
        complete = resolve;
      }),
  );
  const load = vi.spyOn(acpDesktop, "getChatStatus");
  const hook = renderHook(() => useAgentRailStatus(true));
  hook.unmount();
  const dispose = vi.fn();
  await act(async () => complete(dispose));
  expect(dispose).toHaveBeenCalledOnce();
  expect(load).not.toHaveBeenCalled();
});

it("reads only bounded status during streaming and sleeps with the hidden rail", async () => {
  seed();
  let notify!: (event: AcpProjectionChangedEvent) => void;
  const dispose = vi.fn();
  const subscribe = vi
    .spyOn(acpDesktop, "subscribeProjectionChanges")
    .mockImplementation(async (callback) => {
      notify = callback;
      return dispose;
    });
  const pages = vi.spyOn(acpDesktop, "getChatPage");
  const load = vi
    .spyOn(acpDesktop, "getChatStatus")
    .mockImplementation(async (request) => ({
      ...status("running"),
      chatId: request.chatId,
    }));
  const hook = renderHook(({ visible }) => useAgentRailStatus(visible), {
    initialProps: { visible: false },
  });
  expect(subscribe).not.toHaveBeenCalled();
  hook.rerender({ visible: true });
  await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
  const event = {
    projectId: "project-1",
    chatId: "chat-1",
  } as AcpProjectionChangedEvent;
  act(() => {
    for (let index = 0; index < 200; index++) notify(event);
  });
  await waitFor(() => expect(load).toHaveBeenCalledTimes(3));
  expect(pages).not.toHaveBeenCalled();
  act(() => notify(event));
  hook.rerender({ visible: false });
  expect(dispose).toHaveBeenCalledOnce();
  await new Promise((resolve) => setTimeout(resolve, 100));
  expect(load).toHaveBeenCalledTimes(3);
});
