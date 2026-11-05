import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useAgentChangesPane } from "./useAgentChangesPane";
import { useAgentChangesState } from "./agentChangesState";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";

afterEach(() => {
  cleanup();
  useAgentChangesState.getState().close();
  useAgentArtifactFollowStore.getState().clear();
});
it("scopes the Changes tab to its Chat, session and project lifetime", () => {
  const owner = {
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
  } as AgentArtifactOwner;
  const expand = vi.fn();
  const { result, rerender } = renderHook(
    ({ generation, chatId, sessionId }) =>
      useAgentChangesPane(
        { ...owner, chatId, sessionId },
        generation,
        false,
        expand,
      ),
    { initialProps: { generation: 1, chatId: "chat", sessionId: "session" } },
  );
  act(() => useAgentChangesState.getState().show(owner, 1));
  expect(result.current.selected).toBe(true);
  expect(expand).toHaveBeenCalledWith(true);
  act(() =>
    useAgentArtifactFollowStore
      .getState()
      .pin(owner, { kind: "tab", tabId: "other-file" }),
  );
  expect(result.current.open).toBe(true);
  expect(result.current.surface).toBeNull();
  rerender({ generation: 1, chatId: "other", sessionId: "session" });
  expect(result.current.open).toBe(false);
  rerender({ generation: 1, chatId: "chat", sessionId: "other-session" });
  expect(result.current.open).toBe(false);
  rerender({ generation: 2, chatId: "chat", sessionId: "session" });
  expect(result.current.open).toBe(false);
});
