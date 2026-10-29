import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, AcpProjectionChangedEvent } from "../domain/acp";
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
  let current = page("running");
  const load = vi
    .spyOn(acpDesktop, "getChatPage")
    .mockImplementation(async (request) => ({
      ...current,
      chatId: request.chatId,
    }));
  const hook = renderHook(() => {
    useAgentRailStatus();
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
    sessionId: current.currentSessionId!,
    projectionRevision: "2" as never,
  };
  current = page("waitingApproval");
  act(() => {
    for (let index = 0; index < 100; index += 1) notify(event);
  });
  await waitFor(() =>
    expect(hook.result.current.approval.has("chat-1")).toBe(true),
  );
  expect(load).toHaveBeenCalledTimes(3);
  current = page("completed");
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
  let resolve!: (value: AcpChatPage) => void;
  vi.spyOn(acpDesktop, "getChatPage").mockImplementation(
    () =>
      new Promise((complete) => {
        resolve = complete;
      }),
  );
  const hook = renderHook(() => useAgentRailStatus());
  await waitFor(() => expect(resolve).toBeDefined());
  act(() =>
    useWorkbenchStore.setState({
      projectGeneration: 8,
      projectTransitioning: true,
    }),
  );
  await act(async () => resolve(page("waitingApproval")));
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
  let complete!: (value: AcpChatPage) => void;
  const load = vi
    .spyOn(acpDesktop, "getChatPage")
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    )
    .mockResolvedValue(page("completed"));
  renderHook(() => useAgentRailStatus());
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
  await act(async () => complete(page("running")));
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
  const load = vi.spyOn(acpDesktop, "getChatPage");
  const hook = renderHook(() => useAgentRailStatus());
  hook.unmount();
  const dispose = vi.fn();
  await act(async () => complete(dispose));
  expect(dispose).toHaveBeenCalledOnce();
  expect(load).not.toHaveBeenCalled();
});
