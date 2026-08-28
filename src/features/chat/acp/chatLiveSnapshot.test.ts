import { describe, expect, it } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import {
  MAX_LIVE_SNAPSHOT_TEXT,
  projectChatLiveSnapshot,
} from "./chatLiveSnapshot";

function item(text: string, ordinal: number): AcpItem {
  return {
    id: `item-${ordinal}`,
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "1",
    ordinal,
    origin: "primary",
    activityId: null,
    lifecycle: "streaming",
    revision: "1",
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: null,
    kind: "text",
    toolState: null,
    payload: { role: "assistant", text },
  } as AcpItem;
}

function page(turns: TurnProjection[]): AcpChatPage {
  return {
    projectId: "project-1",
    chatId: "chat-1",
    title: "Chat",
    projectionRevision: "1",
    currentSessionId: "session-1",
    sessions: [],
    turns,
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function turn(
  state: TurnProjection["state"],
  ordinal: number,
  items: AcpItem[] = [],
): TurnProjection {
  return {
    id: `turn-${ordinal}`,
    sessionId: "session-1",
    runtimeEpoch: "1",
    ordinal,
    state,
    revision: "1",
    createdAt: ordinal,
    updatedAt: ordinal,
    terminalAt: null,
    items,
    activities: [],
  } as unknown as TurnProjection;
}

describe("bounded Agent chat live snapshot", () => {
  it("labels working and waiting turns without keeping the full transcript", () => {
    const long = "x".repeat(MAX_LIVE_SNAPSHOT_TEXT + 80);
    const snapshot = projectChatLiveSnapshot(
      "chat-1",
      page([
        turn("completed", 1, [item("older completed line", 1)]),
        turn("running", 2, [item("first", 1), item(long, 2)]),
      ]),
    );
    expect(snapshot.working).toBe(true);
    expect(snapshot.waiting).toBe(false);
    expect(snapshot.streaming).toBe(true);
    expect(snapshot.statusLabel).toBe("Running");
    expect(snapshot.latestText.length).toBe(MAX_LIVE_SNAPSHOT_TEXT);
    expect(snapshot.latestText.endsWith("…")).toBe(true);
    expect(snapshot.latestText.includes("older completed")).toBe(false);
  });

  it("marks waiting-for-user turns and idle completed chats", () => {
    expect(
      projectChatLiveSnapshot(
        "chat-1",
        page([turn("waitingApproval", 1, [item("Need a decision", 1)])]),
      ),
    ).toMatchObject({
      waiting: true,
      working: false,
      statusLabel: "Waiting for approval",
      latestText: "Need a decision",
    });
    expect(
      projectChatLiveSnapshot("chat-1", page([turn("completed", 1)])),
    ).toMatchObject({
      waiting: false,
      working: false,
      streaming: false,
      statusLabel: "Completed",
      latestText: "",
    });
  });
});
