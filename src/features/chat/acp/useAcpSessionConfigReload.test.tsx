import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useAcpProjectionStore } from "../../../app/acpProjectionStore";
import { useWorkbenchStore } from "../../../app/store";
import type {
  AcpChatPage,
  AcpSessionConfigChangedV1,
} from "../../../domain/acp";
import { acpDesktop } from "../../../services/acpDesktop";
import { useAcpChatProjection } from "./useAcpChatProjection";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  useAcpProjectionStore.getState().clear();
});
it("coalesces config wake hints into a 50ms authoritative reload without durable revision change", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-a" } as never,
    projectGeneration: 7,
    nativeTransitionEpoch: 3,
    projectTransitioning: false,
    activeTabId: "chat-a",
    tabs: [{ id: "chat-a", kind: "chat" }] as never,
  });
  const base = {
    projectId: "project-a",
    chatId: "chat-a",
    title: "A",
    projectionRevision: "10",
    currentSessionId: "session-a",
    sessions: [
      {
        id: "session-a",
        runtimeEpoch: "2",
        state: "ready",
        residency: "hot",
        capabilities: { configCount: 0 },
      },
    ],
    turns: [],
    nextCursor: null,
    sessionConfig: {
      sageAgentSessionId: "session-a",
      runtimeEpoch: "2",
      configRevision: "1",
      modes: {
        currentModeId: "ask",
        availableModes: [
          { id: "ask", name: "Ask" },
          { id: "code", name: "Code" },
        ],
      },
      configOptions: [],
    },
  } as unknown as AcpChatPage;
  const latest = {
    ...base,
    sessionConfig: {
      ...base.sessionConfig!,
      configRevision: "3",
      modes: { ...base.sessionConfig!.modes!, currentModeId: "code" },
    },
  };
  let onConfig!: (event: AcpSessionConfigChangedV1) => void;
  vi.spyOn(acpDesktop, "subscribeProjectionChanges").mockResolvedValue(
    () => undefined,
  );
  vi.spyOn(acpDesktop, "subscribeSessionConfigChanges").mockImplementation(
    async (callback) => {
      onConfig = callback;
      return () => undefined;
    },
  );
  const load = vi
    .spyOn(acpDesktop, "getChatPage")
    .mockResolvedValueOnce(base)
    .mockResolvedValue(latest);
  const { result } = renderHook(() => useAcpChatProjection());
  await waitFor(() =>
    expect(result.current?.sessionConfig?.configRevision).toBe("1"),
  );
  const notice = {
    schemaVersion: 1 as const,
    sageProjectId: "project-a",
    sageChatId: "chat-a",
    sageAgentSessionId: "session-a",
    runtimeEpoch: "2",
    configRevision: "2",
  };
  act(() => {
    onConfig(notice);
    onConfig({ ...notice, configRevision: "3" });
  });
  expect(result.current?.sessionConfig?.modes?.currentModeId).toBe("ask");
  expect(load).toHaveBeenCalledTimes(1);
  await waitFor(() =>
    expect(result.current?.sessionConfig?.configRevision).toBe("3"),
  );
  expect(load).toHaveBeenCalledTimes(2);
  expect(result.current?.projectionRevision).toBe("10");
  expect(result.current?.sessionConfig?.modes?.currentModeId).toBe("code");
});
