import { describe, expect, it } from "vitest";

import type {
  AcpChatPage,
  AcpItem,
  RuntimeEpoch,
  SageAgentSessionId,
  TurnProjection,
} from "./acp";
import { agentSessionFixture } from "./acpProjection.fixture";
import {
  MAX_HANDOFF_CONTEXT_BYTES,
  MAX_HANDOFF_MESSAGE_BYTES,
} from "./chatHandoff";
import {
  continuedNoticeBefore,
  decodeSessionBindingNotice,
  refusedSessionBinding,
  sessionContinuationDraft,
  type SessionBindingNotice,
} from "./sessionBinding";
import { utf8ByteLength } from "./text";

function text(
  turnId: string,
  role: "user" | "assistant",
  value: string,
): AcpItem {
  return {
    id: `${turnId}-${role}`,
    origin: "primary",
    kind: "text",
    payload: { role, text: value },
  } as unknown as AcpItem;
}

function turn(
  ordinal: number,
  sessionId: string,
  runtimeEpoch: string,
  items: AcpItem[] = [],
): TurnProjection {
  return {
    id: `turn-${ordinal}`,
    sessionId,
    runtimeEpoch,
    ordinal,
    items,
  } as unknown as TurnProjection;
}

function page(
  notice: SessionBindingNotice | undefined,
  turns: TurnProjection[],
  currentSessionId: string | null = "session-a",
): AcpChatPage {
  return {
    projectId: "project-a",
    chatId: "chat-a",
    title: "Drift",
    projectionRevision: "4",
    currentSessionId,
    sessions: [agentSessionFixture(notice ? { bindingNotice: notice } : {})],
    turns,
    nextCursor: null,
  } as unknown as AcpChatPage;
}

const continued: SessionBindingNotice = {
  kind: "continued",
  runtimeEpoch: "3" as RuntimeEpoch,
  change: "compatibleAdapter",
  fromVersion: "0.76.0",
  toVersion: "0.101.0",
};

const refused: SessionBindingNotice = {
  kind: "refused",
  reason: "unknownBuild",
  fromVersion: "0.101.0",
  toVersion: "0.102.0",
};

describe("session binding notices", () => {
  it("decodes the projected notices and rejects a malformed one", () => {
    expect(
      decodeSessionBindingNotice({
        kind: "continued",
        runtimeEpoch: "3",
        change: "compatibleAdapter",
        fromVersion: "0.76.0",
        toVersion: "0.101.0",
      }),
    ).toEqual(continued);
    expect(
      decodeSessionBindingNotice({
        kind: "refused",
        reason: "unknownBuild",
        fromVersion: "0.101.0",
        toVersion: "0.102.0",
      }),
    ).toEqual(refused);
    for (const malformed of [
      { ...continued, runtimeEpoch: "0" },
      { ...continued, change: "anything" },
      { ...refused, reason: "stale" },
      { ...refused, toVersion: "" },
      { ...refused, toVersion: "9".repeat(129) },
    ]) {
      expect(decodeSessionBindingNotice(malformed)).toBeUndefined();
    }
  });

  it("shows a continued drift once, above the first Turn of its epoch", () => {
    const turns = [
      turn(1, "session-a", "2"),
      turn(2, "session-a", "3"),
      turn(3, "session-a", "3"),
      turn(4, "session-a", "4"),
    ];
    const current = page(continued, turns);
    expect(
      turns.map((candidate) => continuedNoticeBefore(current, candidate)),
    ).toEqual([
      null,
      "The agent adapter was updated (0.76.0 → 0.101.0). This chat continues in the same session.",
      null,
      null,
    ]);
  });

  it("offers recovery only when the refused session is no longer current", () => {
    expect(refusedSessionBinding(page(refused, [], null))).toEqual(refused);
    expect(refusedSessionBinding(page(refused, [], "session-a"))).toBeNull();
    expect(refusedSessionBinding(page(continued, [], null))).toBeNull();
    const replaced = {
      ...page(refused, [], null),
      sessions: [
        agentSessionFixture({ bindingNotice: refused }),
        agentSessionFixture({
          id: "session-b" as SageAgentSessionId,
          ordinal: 1,
          state: "closed",
        }),
      ],
    };
    expect(refusedSessionBinding(replaced)).toBeNull();
  });

  it("summarises the newest visible messages within the handoff limits", () => {
    const long = "x".repeat(MAX_HANDOFF_MESSAGE_BYTES * 2);
    const turns = Array.from({ length: 12 }, (_, index) =>
      turn(index, "session-a", "1", [
        text(`turn-${index}`, "user", `question ${index}`),
        text(
          `turn-${index}`,
          "assistant",
          index === 11 ? long : `answer ${index}`,
        ),
      ]),
    );
    const draft = sessionContinuationDraft("Drift", page(refused, turns, null));
    expect(draft).toContain('Chat handoff from "Drift"');
    expect(draft).toContain("User: question 11");
    expect(draft).not.toContain("question 0");
    expect(draft).not.toContain(long);
    expect(utf8ByteLength(draft)).toBeLessThan(MAX_HANDOFF_CONTEXT_BYTES * 2);
  });
});
