import { describe, expect, it } from "vitest";

import type { AgentRailChat } from "../domain/workbench";
import {
  agentRailTrailingState,
  compareAgentRailViewChats,
  railChatAge,
} from "./agentRailStatusView";

function chat(id: string, position: number, updatedAt?: number): AgentRailChat {
  return { id, title: id, pinned: false, position, updatedAt };
}

describe("agent rail status view", () => {
  it("chooses one trailing state: attention, then activity, then unread", () => {
    expect(agentRailTrailingState({ status: "failed", unread: true })).toEqual({
      kind: "failed",
      label: "Failed",
    });
    expect(agentRailTrailingState({ status: "waiting", unread: true })).toEqual(
      { kind: "waiting", label: "Waiting" },
    );
    expect(agentRailTrailingState({ status: "working", unread: true })).toEqual(
      { kind: "working", label: "Working…" },
    );
    expect(
      agentRailTrailingState({ status: "recovering", unread: true }),
    ).toEqual({ kind: "recovering", label: "Recovering" });
    expect(
      agentRailTrailingState({ status: "complete", unread: true }),
    ).toEqual({ kind: "unread", label: "Unread" });
    expect(
      agentRailTrailingState({ status: "idle", unread: false }),
    ).toBeNull();
  });

  it("orders by last update, or by the person's own order", () => {
    const chats = [chat("old", 0, 10), chat("new", 2, 30), chat("mid", 1, 20)];
    const ids = (ordering: "recency" | "manual") =>
      chats
        .slice()
        .sort((left, right) => compareAgentRailViewChats(left, right, ordering))
        .map((entry) => entry.id);
    expect(ids("recency")).toEqual(["new", "mid", "old"]);
    expect(ids("manual")).toEqual(["old", "mid", "new"]);
  });
});

describe("railChatAge", () => {
  const now = Date.UTC(2026, 10, 28, 12);
  it.each([
    [now - 20_000, "now"],
    [now - 3 * 60_000, "3m"],
    [now - 59 * 60_000, "59m"],
    [now - 2 * 3_600_000, "2h"],
    [now - 26 * 3_600_000, "1d"],
    [now - 6 * 86_400_000, "6d"],
    [now - 22 * 86_400_000, "3w"],
    [now + 60_000, "now"],
  ])("shows %i as %s", (updatedAt, age) => {
    expect(railChatAge(updatedAt, now)).toBe(age);
  });

  it("shows nothing without a usable time", () => {
    expect(railChatAge(undefined, now)).toBeNull();
    expect(railChatAge(0, now)).toBeNull();
    expect(railChatAge(Number.NaN, now)).toBeNull();
  });
});
