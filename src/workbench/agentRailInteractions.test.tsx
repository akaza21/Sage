import {
  act,
  cleanup,
  render,
  renderHook,
  screen,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useWorkbenchStore } from "../app/store";
import { AgentRailArchiveNotice } from "./AgentRailArchiveNotice";
import {
  ARCHIVE_UNDO_MS,
  pruneAgentRailInteractions,
  resetAgentRailInteractions,
  setAgentRailApproval,
  setAgentRailArchiveUndo,
  useAgentRailInteractions,
} from "./agentRailInteractions";
afterEach(() => {
  cleanup();
  resetAgentRailInteractions();
  vi.useRealTimers();
});
it("expires an archive undo", () => {
  vi.useFakeTimers();
  const hook = renderHook(useAgentRailInteractions);
  act(() =>
    setAgentRailArchiveUndo({ projectId: "p", chatId: "c", title: "Chat" }),
  );
  act(() => vi.advanceTimersByTime(ARCHIVE_UNDO_MS));
  expect(hook.result.current.undo).toBeNull();
});
it("admits the newest approvals at capacity and prunes closed chats", () => {
  const hook = renderHook(useAgentRailInteractions);
  act(() => {
    for (let index = 0; index < 65; index++)
      setAgentRailApproval(`chat-${index}`, true);
  });
  expect(hook.result.current.approval.size).toBe(64);
  expect(hook.result.current.approval.has("chat-64")).toBe(true);
  act(() => pruneAgentRailInteractions(new Set(["chat-64"])));
  expect([...hook.result.current.approval]).toEqual(["chat-64"]);
});

it("removes archive Undo after a manual restore is observed", () => {
  const initial = useWorkbenchStore.getState();
  useWorkbenchStore.setState({
    project: { id: "p", name: "P", roots: [] },
    projectTransitioning: false,
    archivedChats: [{ id: "c", title: "Chat" }] as never,
  });
  setAgentRailArchiveUndo({ projectId: "p", chatId: "c", title: "Chat" });
  render(<AgentRailArchiveNotice />);
  expect(screen.getByRole("button", { name: "Undo archive" })).toBeVisible();
  act(() => useWorkbenchStore.setState({ archivedChats: [] }));
  expect(screen.queryByRole("button", { name: "Undo archive" })).toBeNull();
  useWorkbenchStore.setState(initial, true);
});
