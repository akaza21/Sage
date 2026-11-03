import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useAgentChangesPane } from "./useAgentChangesPane";
import { useAgentChangesState } from "./agentChangesState";
import type { AgentArtifactOwner } from "../domain/agentArtifact";

afterEach(() => {
  cleanup();
  useAgentChangesState.getState().close();
});
it("scopes the Changes tab to its Chat, session and project lifetime", () => {
  const owner = {
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
  } as AgentArtifactOwner;
  const expand = vi.fn();
  const { result, rerender } = renderHook(
    ({ generation, chatId }) =>
      useAgentChangesPane(owner, generation, false, expand, "project", chatId),
    { initialProps: { generation: 1, chatId: "chat" } },
  );
  act(() => useAgentChangesState.getState().show(owner, 1));
  expect(result.current.selected).toBe(true);
  expect(expand).toHaveBeenCalledWith(true);
  act(() => useAgentChangesState.getState().select(false));
  expect(result.current.open).toBe(true);
  expect(result.current.surface).toBeNull();
  rerender({ generation: 1, chatId: "other" });
  expect(result.current.open).toBe(false);
  rerender({ generation: 2, chatId: "chat" });
  expect(result.current.open).toBe(false);
});
