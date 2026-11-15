import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useAgentChangesPane, useChangesOnExpand } from "./useAgentChangesPane";
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

it("opens Changes once the owner loads after the pane expands", () => {
  const owner = {
    projectId: "project",
    chatId: "chat",
    sessionId: "session",
  } as AgentArtifactOwner;
  const { rerender } = renderHook(
    ({ expanded, current }) => useChangesOnExpand(expanded, true, current, 1),
    {
      initialProps: {
        expanded: false,
        current: null as AgentArtifactOwner | null,
      },
    },
  );
  rerender({ expanded: true, current: null });
  expect(useAgentChangesState.getState().owner).toBeNull();
  rerender({ expanded: true, current: owner });
  expect(useAgentChangesState.getState()).toMatchObject({
    owner,
    selected: true,
  });
  act(() => useAgentChangesState.getState().close());
  rerender({ expanded: true, current: { ...owner } });
  expect(useAgentChangesState.getState().owner).toBeNull();
  rerender({ expanded: false, current: null });
  rerender({ expanded: true, current: null });
  rerender({ expanded: false, current: null });
  rerender({ expanded: false, current: owner });
  expect(useAgentChangesState.getState().owner).toBeNull();
});

it("opens a sessionless Chat's review and keeps it once a session starts", () => {
  const { result, rerender } = renderHook(
    ({ owner }) =>
      useAgentChangesPane(owner, 1, true, undefined, "project", "chat", true),
    { initialProps: { owner: null as AgentArtifactOwner | null } },
  );
  expect(result.current.owner).toEqual({
    projectId: "project",
    chatId: "chat",
    sessionId: null,
  });
  act(() => useAgentChangesState.getState().show(result.current.owner!, 1));
  expect(result.current.selected).toBe(true);
  rerender({
    owner: {
      projectId: "project",
      chatId: "chat",
      sessionId: "session",
    } as AgentArtifactOwner,
  });
  expect(result.current.selected).toBe(true);
  expect(result.current.owner?.sessionId).toBe("session");
});
