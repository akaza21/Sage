import { describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem } from "../../../domain/acp";
import { currentChatUsage } from "./chatUsage";

function usage(
  id: string,
  ordinal: number,
  totalTokens: number | null,
  sessionId = "session-1",
  contextWindow?: number,
): Extract<AcpItem, { kind: "usage" }> {
  return {
    id,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId,
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: ordinal,
    kind: "usage",
    toolState: null,
    payload: {
      inputTokens: totalTokens,
      outputTokens: null,
      cachedInputTokens: null,
      totalTokens,
      ...(contextWindow === undefined ? {} : { contextWindow }),
    },
  } as Extract<AcpItem, { kind: "usage" }>;
}

function page(
  items: readonly AcpItem[],
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
        items,
        activities: [],
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

describe("currentChatUsage", () => {
  it("selects the latest reported total for the current session", () => {
    expect(
      currentChatUsage(page([usage("one", 1, 1_024), usage("two", 2, 2_048)])),
    ).toMatchObject({
      totalTokens: 2_048,
    });
  });

  it("omits usage when the current binding did not report a total", () => {
    expect(currentChatUsage(page([usage("unknown", 1, null)]))).toBeNull();
    expect(currentChatUsage(null)).toBeNull();
  });

  it("keeps runtime context capacity on the selected usage snapshot", () => {
    expect(
      currentChatUsage(page([usage("known", 1, 6_000, "session-1", 272_000)])),
    ).toMatchObject({ totalTokens: 6_000, contextWindow: 272_000 });
  });

  it("uses the last current-session usage across turn-local ordinals", () => {
    const first = page([usage("long-turn", 19, 8_000)]);
    first.turns = [
      ...first.turns,
      {
        ...first.turns[0]!,
        id: "turn-2" as never,
        ordinal: 2,
        items: [usage("short-turn", 4, 12_000)],
      },
    ];
    expect(currentChatUsage(first)).toMatchObject({ totalTokens: 12_000 });
  });

  it("does not use usage from a different session", () => {
    expect(
      currentChatUsage(
        page(
          [usage("foreign", 1, 4_096, "session-2")],
          "session-1",
          "session-2",
        ),
      ),
    ).toBeNull();
  });
});
