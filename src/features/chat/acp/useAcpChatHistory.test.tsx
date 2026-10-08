import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type { AcpChatPage } from "../../../domain/acp";
import { decodeAcpGoal } from "../../../domain/acpGoal";
import { acpDesktop } from "../../../services/acpDesktop";
import { useAcpChatHistory } from "./useAcpChatHistory";

const page = {
  projectId: "project-1",
  chatId: "chat-1",
  title: "Durable chat",
  projectionRevision: "4",
  currentSessionId: null,
  sessions: [],
  turns: [],
  nextCursor: "cursor-1",
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
  const scope = {
    projectId: page.projectId,
    projectGeneration: "7" as never,
    chatId: page.chatId,
  };
  useAcpProjectionStore.getState().activateScope(scope);
  useAcpProjectionStore.getState().applyAuthoritativePage(scope, page);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
});

describe("useAcpChatHistory", () => {
  it("loads only the user-selected bounded continuation cursor", async () => {
    const load = vi.spyOn(acpDesktop, "getChatPage").mockResolvedValue({
      ...page,
      nextCursor: null,
    });
    const { result } = renderHook(() => useAcpChatHistory());

    act(() => result.current.loadEarlier());
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));
    expect(load).toHaveBeenCalledWith(
      {
        projectId: "project-1",
        projectGeneration: "3",
        chatId: "chat-1",
        cursor: "cursor-1",
      },
      decodeAcpGoal,
    );
    await waitFor(() => expect(result.current.hasEarlier).toBe(false));
  });
});
