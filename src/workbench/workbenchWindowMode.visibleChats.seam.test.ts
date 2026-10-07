import { describe, expect, it } from "vitest";

import {
  MAX_AGENT_VISIBLE_CHATS,
  type AgentCenterScanTab,
  collectChatTabIds,
  focusedAgentChatId,
  lastArtifactTabId,
  reconcileVisibleChatIds,
  selectVisibleChatIds,
} from "./workbenchWindowMode";

describe("Agent visible chat selection", () => {
  it("collects and selects the last native-admitted tab", () => {
    const tabs: AgentCenterScanTab[] = Array.from(
      { length: 63 },
      (_, index) => ({
        id: `file:${index}`,
        kind: "file",
      }),
    );
    tabs.push({ id: "chat:late", kind: "chat" });
    expect(collectChatTabIds(tabs)).toEqual(["chat:late"]);
    expect(selectVisibleChatIds(["chat:late"], [], "chat:late")).toEqual([
      "chat:late",
    ]);
    const manyChats: AgentCenterScanTab[] = Array.from(
      { length: 64 },
      (_, index) => ({ id: `chat:${index}`, kind: "chat" }),
    );
    const ids = collectChatTabIds(manyChats);
    expect(ids).toHaveLength(64);
    expect(selectVisibleChatIds(ids, ["chat:0", "chat:1"], "chat:63")).toEqual([
      "chat:1",
      "chat:63",
    ]);
  });

  it("caps concurrent visible chats at two", () => {
    expect(MAX_AGENT_VISIBLE_CHATS).toBe(2);
  });

  it("collects only in-bound chat tabs", () => {
    const tabs: AgentCenterScanTab[] = [
      { id: "file:a", kind: "file" },
      { id: "chat:a", kind: "chat" },
      { id: "chat:b", kind: "chat" },
      { id: "diff:d", kind: "diff" },
      { id: "chat:c", kind: "chat" },
    ];
    expect(collectChatTabIds(tabs)).toEqual(["chat:a", "chat:b", "chat:c"]);
  });

  it("seeds the first two chats when nothing is visible yet", () => {
    expect(
      reconcileVisibleChatIds(["chat:a", "chat:b", "chat:c"], [], null),
    ).toEqual(["chat:a", "chat:b"]);
  });

  it("keeps a focused chat visible and drops missing ids", () => {
    expect(
      reconcileVisibleChatIds(
        ["chat:b", "chat:c"],
        ["chat:a", "chat:b"],
        "chat:c",
      ),
    ).toEqual(["chat:b", "chat:c"]);
  });

  it("adds a third selected chat by replacing the oldest visible pane", () => {
    expect(
      selectVisibleChatIds(
        ["chat:a", "chat:b", "chat:c"],
        ["chat:a", "chat:b"],
        "chat:c",
      ),
    ).toEqual(["chat:b", "chat:c"]);
  });

  it("fills a second pane when focusing one chat from an empty visible set", () => {
    expect(
      selectVisibleChatIds(["chat:a", "chat:b", "chat:c"], [], "chat:a"),
    ).toEqual(["chat:a", "chat:b"]);
  });

  it("does not duplicate a chat that is already visible", () => {
    expect(
      selectVisibleChatIds(
        ["chat:a", "chat:b"],
        ["chat:a", "chat:b"],
        "chat:a",
      ),
    ).toEqual(["chat:a", "chat:b"]);
  });

  it("focuses the active chat when it is one of the chats", () => {
    expect(
      focusedAgentChatId(
        ["chat:a", "chat:b"],
        ["chat:a", "chat:b"],
        "chat:b",
        "chat:a",
      ),
    ).toBe("chat:b");
    expect(
      focusedAgentChatId(
        ["chat:a", "chat:b"],
        ["chat:a", "chat:b"],
        "file:a",
        "chat:b",
      ),
    ).toBe("chat:b");
    expect(focusedAgentChatId(["chat:a"], ["chat:a"], "file:a", "chat:b")).toBe(
      "chat:a",
    );
  });

  it("prefers the remembered artifact for Editor continuity", () => {
    const tabs: AgentCenterScanTab[] = [
      { id: "file:a", kind: "file" },
      { id: "chat:b", kind: "chat" },
      { id: "diff:d", kind: "diff" },
    ];
    expect(lastArtifactTabId(tabs, "file:a")).toBe("file:a");
    expect(lastArtifactTabId(tabs, "chat:b")).toBe("diff:d");
    expect(lastArtifactTabId(tabs, null)).toBe("diff:d");
  });
});
