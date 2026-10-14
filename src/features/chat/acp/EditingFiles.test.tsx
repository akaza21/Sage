import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { AcpItem } from "../../../domain/acp";
import { AcpChatPresentation } from "./AcpChatPresentation";
import { page } from "./acpProjection.fixture";

afterEach(cleanup);

it("shows Editing files only while a genuinely active owned edit is in flight", () => {
  const projection = page("running");
  const edit = {
    id: "live-edit",
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "2",
    revision: "1",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    createdAt: 1,
    updatedAt: 1,
    terminalAt: null,
    kind: "fileChangeDiff",
    lifecycle: "streaming",
    toolState: null,
    payload: { changes: [], truncated: false },
  } as unknown as AcpItem;
  projection.turns[0].items = [edit];
  const props = {
    page: projection,
    hasEarlier: false,
    loadingEarlier: false,
    historyError: null,
    projectRoots: [],
    retentionIncomplete: false,
  };
  const { rerender } = render(<AcpChatPresentation {...props} />);
  expect(screen.getByText("Editing files")).toBeVisible();
  const liveRow = () => screen.getByText("Editing files").parentElement;
  const announcement = liveRow();
  expect(announcement).toHaveAttribute("role", "status");
  expect(announcement).toHaveAttribute("aria-live", "polite");
  expect(announcement).toHaveTextContent("Editing files");
  const delta = structuredClone(projection);
  delta.turns[0].revision = "2" as never;
  delta.turns[0].items = [{ ...edit, revision: "2" } as AcpItem];
  rerender(<AcpChatPresentation {...props} page={delta} />);
  expect(liveRow()).toBe(announcement);
  let revision = 2;
  for (const lifecycle of ["completed", "failed", "cancelled"] as const) {
    const next = structuredClone(projection);
    next.turns[0].revision = String(++revision) as never;
    next.turns[0].items = [
      { ...edit, lifecycle, revision: next.turns[0].revision } as AcpItem,
    ];
    rerender(<AcpChatPresentation {...props} page={next} />);
    expect(screen.queryByText("Editing files")).toBeNull();
  }
  const foreign = structuredClone(projection);
  foreign.turns[0].revision = "6" as never;
  foreign.turns[0].items = [{ ...edit, chatId: "another-chat" } as AcpItem];
  rerender(<AcpChatPresentation {...props} page={foreign} />);
  expect(screen.queryByText("Editing files")).toBeNull();
  const terminal = structuredClone(projection);
  terminal.turns[0].state = "completed";
  terminal.turns[0].revision = "7" as never;
  rerender(<AcpChatPresentation {...props} page={terminal} />);
  expect(screen.queryByText("Editing files")).toBeNull();
  rerender(<AcpChatPresentation {...props} />);
  expect(liveRow()).not.toBe(announcement);
  rerender(
    <AcpChatPresentation {...props} page={{ ...projection, sessions: [] }} />,
  );
  expect(screen.queryByText("Editing files")).toBeNull();
});
