import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { chatAttentionStore } from "../app/chatAttentionStore";
import { attention } from "../domain/chatAttention.fixture";
import { acpDesktop } from "../services/acpDesktop";
import { chatAttention } from "../services/chatAttention";
import { useAgentRailStatus } from "./useAgentRailStatus";

vi.mock("../ports/desktopRuntime", () => ({ isTauriRuntime: () => true }));
const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  chatAttentionStore.disconnect();
  vi.restoreAllMocks();
  useWorkbenchStore.setState(initial, true);
});

it("attention replaces transcript fetching for unopened chats", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1", name: "Project", roots: [] },
    projects: [],
    projectTransitioning: false,
    tabs: ["chat-1", "chat-2"].map((id, position) => ({
      id,
      position,
      kind: "chat",
      title: "Chat",
      pinned: false,
      dirty: false,
    })),
  });
  const transcript = vi.spyOn(acpDesktop, "getChatPage");
  const subscribe = vi
    .spyOn(chatAttention, "subscribe")
    .mockResolvedValue(() => undefined);
  const get = vi
    .spyOn(chatAttention, "get")
    .mockResolvedValue([
      attention({ pendingApprovals: 1 }),
      attention({ chatId: "chat-2", terminalState: "failed" }),
    ]);
  renderHook(() => useAgentRailStatus());
  await waitFor(() =>
    expect(
      chatAttentionStore.getSnapshot().get("chat-1")?.pendingApprovals,
    ).toBe(1),
  );
  expect(chatAttentionStore.getSnapshot().get("chat-2")?.terminalState).toBe(
    "failed",
  );
  expect(subscribe.mock.invocationCallOrder[0]).toBeLessThan(
    get.mock.invocationCallOrder[0],
  );
  expect(transcript).not.toHaveBeenCalled();
});
