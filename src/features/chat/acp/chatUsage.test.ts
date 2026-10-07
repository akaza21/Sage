import { describe, expect, it } from "vitest";

import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { currentChatUsage } from "./chatUsage";

function usage(
  totalTokens: number,
  contextWindow?: number,
): NonNullable<TurnProjection["usage"]> {
  return {
    inputTokens: totalTokens,
    outputTokens: 0,
    totalTokens,
    ...(contextWindow === undefined ? {} : { contextWindow }),
  };
}

function page(
  usage: TurnProjection["usage"],
  currentSessionId = "session-1",
  turnSessionId = currentSessionId,
): AcpChatPage {
  return {
    projectId: "project-1",
    chatId: "chat-1",
    title: "Chat",
    projectionRevision: "1",
    currentSessionId,
    sessions: [],
    turns: [
      {
        id: "turn-1",
        sessionId: turnSessionId,
        runtimeEpoch: "1",
        ordinal: 1,
        state: "completed",
        revision: "1",
        createdAt: 1,
        updatedAt: 1,
        terminalAt: 1,
        usage,
        items: [],
        activities: [],
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

describe("currentChatUsage", () => {
  it("selects the latest reported total for the current session", () => {
    expect(currentChatUsage(page(usage(2_048)))).toMatchObject({
      totalTokens: 2_048,
    });
  });

  it("omits usage when the current binding did not report a total", () => {
    expect(currentChatUsage(page(undefined))).toBeNull();
    expect(currentChatUsage(null)).toBeNull();
  });

  it("keeps runtime context capacity on the selected usage snapshot", () => {
    expect(currentChatUsage(page(usage(6_000, 272_000)))).toMatchObject({
      totalTokens: 6_000,
      contextWindow: 272_000,
    });
  });

  it("uses the last current-session usage across turn-local ordinals", () => {
    const first = page(usage(8_000));
    first.turns = [
      ...first.turns,
      {
        ...first.turns[0]!,
        id: "turn-2" as never,
        ordinal: 2,
        usage: usage(12_000),
        items: [],
      },
    ];
    expect(currentChatUsage(first)).toMatchObject({ totalTokens: 12_000 });
  });

  it("does not use usage from a different session", () => {
    expect(
      currentChatUsage(page(usage(4_096), "session-1", "session-2")),
    ).toBeNull();
  });
});
