import { describe, expect, it } from "vitest";

import {
  type AgentCenterScanTab,
  collectChatTabIds,
  focusedAgentChatId,
  lastArtifactTabId,
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
    expect(focusedAgentChatId(["chat:late"], "chat:late", null)).toBe(
      "chat:late",
    );
    const manyChats: AgentCenterScanTab[] = Array.from(
      { length: 64 },
      (_, index) => ({ id: `chat:${index}`, kind: "chat" }),
    );
    const ids = collectChatTabIds(manyChats);
    expect(ids).toHaveLength(64);
    expect(focusedAgentChatId(ids, "chat:63", "chat:0")).toBe("chat:63");
    expect(
      collectChatTabIds([...manyChats, { id: "overflow", kind: "chat" }]),
    ).toEqual(ids);
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

  it("focuses only within the admitted visible candidates", () => {
    expect(focusedAgentChatId(["chat:a", "chat:b"], "chat:b", "chat:a")).toBe(
      "chat:b",
    );
    expect(focusedAgentChatId(["chat:a", "chat:b"], "file:a", "chat:b")).toBe(
      "chat:b",
    );
    expect(focusedAgentChatId(["chat:b", "side-x"], "file:a", "chat:a")).toBe(
      "chat:b",
    );
    expect(focusedAgentChatId([], "file:a", "chat:a")).toBeNull();
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
