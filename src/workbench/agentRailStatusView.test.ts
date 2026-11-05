import { describe, expect, it } from "vitest";

import type { AgentRailChat } from "../domain/workbench";
import {
  agentRailTrailingState,
  compareAgentRailViewChats,
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
