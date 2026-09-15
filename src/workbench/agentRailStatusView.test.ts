import { describe, expect, it } from "vitest";

import type { AgentRailProjectGroup } from "./agentChatRail";
import {
  AGENT_RAIL_FILTER_FIELDS,
  AGENT_RAIL_OMITTED_FILTER_FIELDS,
  AGENT_RAIL_OMITTED_SHOW_FIELDS,
  AGENT_RAIL_SHOW_FIELDS,
  MAX_AGENT_RAIL_STATUS_SECTION,
  agentRailAgeStamp,
  agentRailTrailingState,
  classifyAgentRailSection,
  groupAgentRailByStatus,
} from "./agentRailStatusView";

function folder(
  id: string,
  chats: AgentRailProjectGroup["chats"],
  truncated = false,
): AgentRailProjectGroup {
  return {
    id,
    name: id === "sage" ? "Sage" : "Cross",
    rootCount: 1,
    chats,
    archived: [],
    truncated,
  };
}

describe("agent rail status grouping", () => {
  it("classifies durable Sage signals and keeps folder identity", () => {
    expect(classifyAgentRailSection({ status: "working", unread: true })).toBe(
      "working",
    );
    expect(
      classifyAgentRailSection({ status: "recovering", unread: false }),
    ).toBe("working");
    expect(classifyAgentRailSection({ status: "waiting", unread: false })).toBe(
      "attention",
    );
    expect(classifyAgentRailSection({ status: "failed", unread: false })).toBe(
      "attention",
    );
    expect(classifyAgentRailSection({ status: "complete", unread: true })).toBe(
      "attention",
    );
    expect(classifyAgentRailSection({ status: "idle", unread: false })).toBe(
      "done",
    );
    const sections = groupAgentRailByStatus({
      groups: [
        folder("sage", [
          {
            id: "chat-run",
            title: "Kickoff",
            pinned: false,
            position: 0,
            status: "working",
          },
          {
            id: "chat-wait",
            title: "Need a decision",
            pinned: false,
            position: 1,
            status: "waiting",
          },
          {
            id: "chat-done",
            title: "Shipped",
            pinned: false,
            position: 2,
            status: "complete",
          },
          {
            id: "chat-failed",
            title: "Failed after review",
            pinned: false,
            position: 3,
            status: "failed",
          },
        ]),
        folder("cross", [
          {
            id: "chat-old",
            title: "Research",
            pinned: false,
            position: 0,
            status: "idle",
          },
        ]),
      ],
      unreadByChatId: { "chat-done": true },
    });
    expect(sections.map((section) => section.id)).toEqual([
      "attention",
      "working",
      "done",
    ]);
    expect(sections[0]?.groups[0]).toMatchObject({
      id: "sage",
      name: "Sage",
    });
    expect(sections[0]?.groups[0]?.chats.map((chat) => chat.id)).toEqual([
      "chat-wait",
      "chat-done",
      "chat-failed",
    ]);
    expect(sections[1]?.groups[0]?.chats.map((chat) => chat.id)).toEqual([
      "chat-run",
    ]);
    expect(sections[2]?.groups.map((group) => group.id)).toEqual(["cross"]);
    expect(
      groupAgentRailByStatus({
        groups: [
          folder("sage", [
            {
              id: "chat-idle",
              title: "Idle",
              pinned: false,
              position: 0,
              status: "idle",
            },
          ]),
        ],
        unreadByChatId: {},
      }).map((section) => section.id),
    ).toEqual(["done"]);
  });

  it("caps each status section and only admits Sage-owned show/filter fields", () => {
    expect(AGENT_RAIL_SHOW_FIELDS).toEqual(["updated", "workspace", "branch"]);
    expect(AGENT_RAIL_OMITTED_SHOW_FIELDS).toEqual([
      "environment",
      "pr",
      "machine",
    ]);
    expect(AGENT_RAIL_FILTER_FIELDS).toEqual(["status", "archived"]);
    expect(AGENT_RAIL_OMITTED_FILTER_FIELDS).toEqual([
      "pr",
      "environment",
      "source",
    ]);
    const chats = Array.from({ length: 70 }, (_, index) => ({
      id: `chat-${index}`,
      title: `Chat ${index}`,
      pinned: false,
      position: index,
      status: "complete" as const,
    }));
    const [done] = groupAgentRailByStatus({
      groups: [folder("sage", chats)],
      unreadByChatId: {},
      statusFilter: "done",
    }).filter((section) => section.id === "done");
    expect(done?.groups[0]?.chats).toHaveLength(MAX_AGENT_RAIL_STATUS_SECTION);
    expect(done?.truncated).toBe(true);
  });

  it("stamps relative ages from durable clocks and live turns", () => {
    const now = Date.UTC(2026, 7, 26, 18, 0, 0);
    expect(
      agentRailAgeStamp({
        updatedAt: Date.UTC(2026, 7, 26, 12, 0, 0),
        live: false,
        nowMs: now,
      }),
    ).toBe("6h");
    expect(
      agentRailAgeStamp({
        updatedAt: Date.UTC(2026, 6, 4, 12, 0, 0),
        live: false,
        nowMs: now,
      }),
    ).toBe("53d");
    expect(agentRailAgeStamp({ live: true, nowMs: now })).toBe("now");
    expect(agentRailAgeStamp({ live: true, nowMs: 0 })).toBe("");
    expect(agentRailAgeStamp({ live: false, nowMs: now })).toBe("");
  });

  it("chooses one trailing state in attention, activity, unread, then time order", () => {
    expect(
      agentRailTrailingState({ status: "failed", unread: true, stamp: "now" }),
    ).toEqual({ kind: "failed", label: "Failed" });
    expect(
      agentRailTrailingState({ status: "waiting", unread: true, stamp: "now" }),
    ).toEqual({ kind: "waiting", label: "Waiting" });
    expect(
      agentRailTrailingState({ status: "working", unread: true, stamp: "now" }),
    ).toEqual({ kind: "working", label: "Working\u2026" });
    expect(
      agentRailTrailingState({
        status: "recovering",
        unread: true,
        stamp: "now",
      }),
    ).toEqual({ kind: "recovering", label: "Recovering" });
    expect(
      agentRailTrailingState({ status: "complete", unread: true, stamp: "6h" }),
    ).toEqual({ kind: "unread", label: "Unread" });
    expect(
      agentRailTrailingState({ status: "idle", unread: false, stamp: "6h" }),
    ).toEqual({ kind: "time", label: "6h" });
    expect(
      agentRailTrailingState({ status: "idle", unread: false, stamp: "" }),
    ).toBeNull();
  });
});
