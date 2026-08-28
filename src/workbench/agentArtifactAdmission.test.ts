import { describe, expect, it } from "vitest";

import {
  admitAgentArtifactIds,
  agentArtifactAdmitted,
  agentArtifactVisible,
  MAX_AGENT_ADMITTED_ARTIFACTS,
} from "./agentArtifactAdmission";

describe("agent artifact admission", () => {
  it("admits only a bounded Agent tab id and refuses junk", () => {
    expect(admitAgentArtifactIds([], "file:app")).toEqual(["file:app"]);
    expect(admitAgentArtifactIds(["file:app"], "file:app")).toEqual([
      "file:app",
    ]);
    expect(admitAgentArtifactIds(["file:app"], "")).toEqual(["file:app"]);
    expect(admitAgentArtifactIds(["file:app"], "x".repeat(513))).toEqual([
      "file:app",
    ]);
    expect(agentArtifactAdmitted(["file:app"], "file:app")).toBe(true);
    expect(agentArtifactAdmitted(["file:app"], "file:other")).toBe(false);
  });

  it("evicts the oldest id once the cap is reached", () => {
    const filled = Array.from(
      { length: MAX_AGENT_ADMITTED_ARTIFACTS },
      (_, index) => `file:${index}`,
    );
    const next = admitAgentArtifactIds(filled, "file:new");
    expect(next).toHaveLength(MAX_AGENT_ADMITTED_ARTIFACTS);
    expect(next[0]).toBe("file:1");
    expect(next.at(-1)).toBe("file:new");
    expect(agentArtifactAdmitted(next, "file:0")).toBe(false);
  });

  it("shows a tab only when Agent admitted it or the focused chat has stats", () => {
    expect(
      agentArtifactVisible({
        tabId: "file:editor",
        kind: "file",
        admittedIds: [],
        hasFocusedChatStats: false,
      }),
    ).toBe(false);
    expect(
      agentArtifactVisible({
        tabId: "file:agent",
        kind: "file",
        admittedIds: ["file:agent"],
        hasFocusedChatStats: false,
      }),
    ).toBe(true);
    expect(
      agentArtifactVisible({
        tabId: "file:summary",
        kind: "file",
        admittedIds: [],
        hasFocusedChatStats: true,
      }),
    ).toBe(true);
    expect(
      agentArtifactVisible({
        tabId: "chat:a",
        kind: "chat",
        admittedIds: ["chat:a"],
        hasFocusedChatStats: true,
      }),
    ).toBe(false);
  });
});
