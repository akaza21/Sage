import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../domain/acp";
import { agentSessionFixture } from "../../domain/acpProjection.fixture";
import { SessionBindingRecovery } from "./SessionBindingNotices";

afterEach(cleanup);

function refusedPage(currentSessionId: string | null): AcpChatPage {
  const message = {
    id: "item-a",
    origin: "primary",
    kind: "text",
    payload: { role: "user", text: "Keep SQLite authoritative." },
  } as unknown as AcpItem;
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Storage plan",
    projectionRevision: "5",
    currentSessionId,
    sessions: [
      agentSessionFixture({
        state: "incompatible",
        bindingNotice: {
          kind: "refused",
          reason: "otherFamily",
          fromVersion: "0.101.0",
          toVersion: "1.0.0",
        },
      }),
    ],
    turns: [
      {
        id: "turn-a",
        sessionId: "session-a",
        runtimeEpoch: "1",
        ordinal: 0,
        items: [message],
      } as unknown as TurnProjection,
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

it("continues an ended session with a reviewable summary, once", () => {
  const onContinue = vi.fn();
  render(
    <SessionBindingRecovery page={refusedPage(null)} onContinue={onContinue} />,
  );
  expect(
    screen.getByText(
      "The installed adapter (1.0.0) uses a different session protocol than 0.101.0. Your chat history is kept.",
    ),
  ).toBeTruthy();
  const action = screen.getByRole("button", {
    name: "Continue in a new session",
  });
  fireEvent.click(action);
  expect(onContinue).toHaveBeenCalledTimes(1);
  expect(onContinue.mock.calls[0][0]).toContain(
    "User: Keep SQLite authoritative.",
  );
  expect((action as HTMLButtonElement).disabled).toBe(true);
});

it("stays hidden once a new session is current", () => {
  render(
    <SessionBindingRecovery
      page={refusedPage("session-b")}
      onContinue={vi.fn()}
    />,
  );
  expect(screen.queryByRole("button")).toBeNull();
});
