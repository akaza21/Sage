import { describe, expect, it } from "vitest";

import type {
  AgentArtifactAdmission,
  AgentArtifactOwner,
} from "../domain/agentArtifact";

import {
  admitAgentArtifact,
  agentArtifactAdmitted,
  agentArtifactVisible,
  forgetAgentArtifactTab,
  MAX_AGENT_ADMITTED_ARTIFACTS,
} from "./agentArtifactAdmission";

describe("agent artifact admission", () => {
  const ownerA = {
    projectId: "project-a",
    chatId: "chat-a",
    sessionId: "session-a",
  } as AgentArtifactOwner;
  const ownerB = {
    projectId: "project-a",
    chatId: "chat-b",
    sessionId: "session-b",
  } as AgentArtifactOwner;

  it("admits only a bounded exact owner and tab id", () => {
    const admission = { owner: ownerA, tabId: "file:app" };
    expect(admitAgentArtifact([], admission)).toEqual([admission]);
    expect(admitAgentArtifact([admission], admission)).toEqual([admission]);
    expect(
      admitAgentArtifact([admission], { ...admission, tabId: "" }),
    ).toEqual([admission]);
    expect(
      admitAgentArtifact([admission], {
        ...admission,
        tabId: "x".repeat(513),
      }),
    ).toEqual([admission]);
    expect(agentArtifactAdmitted([admission], ownerA, "file:app")).toBe(true);
    expect(agentArtifactAdmitted([admission], ownerB, "file:app")).toBe(false);
  });

  it("evicts the oldest id once the cap is reached", () => {
    const filled: AgentArtifactAdmission[] = Array.from(
      { length: MAX_AGENT_ADMITTED_ARTIFACTS },
      (_, index) => ({ owner: ownerA, tabId: `file:${index}` }),
    );
    const next = admitAgentArtifact(filled, {
      owner: ownerA,
      tabId: "file:new",
    });
    expect(next).toHaveLength(MAX_AGENT_ADMITTED_ARTIFACTS);
    expect(next[0]?.tabId).toBe("file:1");
    expect(next.at(-1)?.tabId).toBe("file:new");
    expect(agentArtifactAdmitted(next, ownerA, "file:0")).toBe(false);
  });

  it("forgets every admission when a shared tab closes or is evicted", () => {
    const admissions = [
      { owner: ownerA, tabId: "file:shared" },
      { owner: ownerB, tabId: "file:shared" },
      { owner: ownerB, tabId: "file:kept" },
    ];
    expect(forgetAgentArtifactTab(admissions, "file:shared")).toEqual([
      { owner: ownerB, tabId: "file:kept" },
    ]);
  });

  it("shows a tab only when Agent admitted it or the focused chat has stats", () => {
    expect(
      agentArtifactVisible({
        tabId: "browser:a",
        kind: "browser",
        admissions: [],
        focusedOwner: ownerA,
        hasFocusedChatStats: false,
      }),
    ).toBe(true);
    expect(
      agentArtifactVisible({
        tabId: "file:editor",
        kind: "file",
        admissions: [],
        focusedOwner: ownerA,
        hasFocusedChatStats: false,
      }),
    ).toBe(false);
    expect(
      agentArtifactVisible({
        tabId: "file:agent",
        kind: "file",
        admissions: [{ owner: ownerA, tabId: "file:agent" }],
        focusedOwner: ownerA,
        hasFocusedChatStats: false,
      }),
    ).toBe(true);
    expect(
      agentArtifactVisible({
        tabId: "file:summary",
        kind: "file",
        admissions: [],
        focusedOwner: ownerA,
        hasFocusedChatStats: true,
      }),
    ).toBe(true);
    expect(
      agentArtifactVisible({
        tabId: "chat:a",
        kind: "chat",
        admissions: [{ owner: ownerA, tabId: "chat:a" }],
        focusedOwner: ownerA,
        hasFocusedChatStats: true,
      }),
    ).toBe(false);
    expect(
      agentArtifactVisible({
        tabId: "file:agent",
        kind: "file",
        admissions: [{ owner: ownerA, tabId: "file:agent" }],
        focusedOwner: ownerB,
        hasFocusedChatStats: false,
      }),
    ).toBe(false);
  });
});
