import { beforeEach, describe, expect, it } from "vitest";

import type { AgentArtifactOwner } from "../domain/agentArtifact";
import {
  agentArtifactFollowRecordFor,
  agentArtifactPendingTargets,
  MAX_AGENT_ARTIFACT_FOLLOW_OWNERS,
  MAX_AGENT_ARTIFACT_TARGETS,
  useAgentArtifactFollowStore,
} from "./agentArtifactFollowStore";

const ownerA = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
} as AgentArtifactOwner;

it("keeps a newly published target when an older availability render reconciles", () => {
  const store = useAgentArtifactFollowStore.getState();
  store.clear();
  store.pin(ownerA, { kind: "subagents" });
  const observed = agentArtifactFollowRecordFor(
    useAgentArtifactFollowStore.getState().records,
    ownerA,
  );
  store.observe(ownerA, { kind: "changes", generation: 1 });
  store.reconcile(ownerA, [{ kind: "subagents" }], observed);
  expect(
    agentArtifactFollowRecordFor(
      useAgentArtifactFollowStore.getState().records,
      ownerA,
    ),
  ).toMatchObject({
    mode: "pinned",
    selected: { kind: "subagents" },
    pending: [{ kind: "changes", generation: 1 }],
  });
  store.followLatest(ownerA);
  expect(
    agentArtifactFollowRecordFor(
      useAgentArtifactFollowStore.getState().records,
      ownerA,
    )?.selected,
  ).toEqual({ kind: "changes", generation: 1 });
});
const ownerB = {
  projectId: "project-a",
  chatId: "chat-b",
  sessionId: "session-b",
} as AgentArtifactOwner;
const ownerC = {
  projectId: "project-b",
  chatId: "chat-c",
  sessionId: "session-c",
} as AgentArtifactOwner;

beforeEach(() => useAgentArtifactFollowStore.getState().clear());

describe("Agent artifact follow state", () => {
  it("follows the latest resource until the user pins an exact target", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.observe(ownerA, { kind: "tab", tabId: "file:a" });
    store.pin(ownerA, { kind: "tab", tabId: "file:a" });
    store.observe(ownerA, { kind: "preview" }, 2);

    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toEqual({
      owner: ownerA,
      mode: "pinned",
      selected: { kind: "tab", tabId: "file:a" },
      recent: [{ kind: "tab", tabId: "file:a" }, { kind: "preview" }],
      pending: [{ kind: "preview" }],
      lastPreviewRevision: 2,
    });

    useAgentArtifactFollowStore.getState().followLatest(ownerA);
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toMatchObject({
      mode: "follow",
      selected: { kind: "preview" },
    });
  });

  it("identifies only a newer target that arrived while pinned", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.observe(ownerA, { kind: "tab", tabId: "file:a" });
    store.pin(ownerA, { kind: "tab", tabId: "file:a" });
    expect(
      agentArtifactPendingTargets(
        agentArtifactFollowRecordFor(
          useAgentArtifactFollowStore.getState().records,
          ownerA,
        ),
      ),
    ).toEqual([]);

    store.observe(ownerA, { kind: "preview" }, 2);
    store.observe(ownerA, { kind: "tab", tabId: "diff:b" });
    store.observe(ownerA, { kind: "tab", tabId: "file:a" });
    expect(
      agentArtifactPendingTargets(
        agentArtifactFollowRecordFor(
          useAgentArtifactFollowStore.getState().records,
          ownerA,
        ),
      ),
    ).toEqual([{ kind: "preview" }, { kind: "tab", tabId: "diff:b" }]);

    store.followLatest(ownerA);
    expect(
      agentArtifactPendingTargets(
        agentArtifactFollowRecordFor(
          useAgentArtifactFollowStore.getState().records,
          ownerA,
        ),
      ),
    ).toEqual([]);
  });

  it("restores independent state for two exact Chat owners sharing a tab", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.pin(ownerA, { kind: "tab", tabId: "file:shared" });
    store.pin(ownerB, { kind: "tab", tabId: "file:shared" });
    store.observe(ownerB, { kind: "preview" }, 4);

    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      )?.selected,
    ).toEqual({ kind: "tab", tabId: "file:shared" });
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerB,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "tab", tabId: "file:shared" },
      lastPreviewRevision: 4,
    });
  });

  it("drops a superseded session without evicting the other visible Chat", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.pin(ownerA, { kind: "tab", tabId: "file:a" });
    store.pin(ownerB, { kind: "tab", tabId: "file:b" });
    const replacement = {
      ...ownerA,
      sessionId: "session-new",
    } as AgentArtifactOwner;
    store.observe(replacement, { kind: "preview" }, 8);

    expect(useAgentArtifactFollowStore.getState().records).toHaveLength(
      MAX_AGENT_ARTIFACT_FOLLOW_OWNERS,
    );
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toBeNull();
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerB,
      )?.selected,
    ).toEqual({ kind: "tab", tabId: "file:b" });
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        replacement,
      )?.selected,
    ).toEqual({ kind: "preview" });
  });

  it("keeps only the two most recently observed exact Chat owners", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.pin(ownerA, { kind: "tab", tabId: "file:a" });
    store.pin(ownerB, { kind: "tab", tabId: "file:b" });
    store.pin(ownerC, { kind: "tab", tabId: "file:c" });

    const records = useAgentArtifactFollowStore.getState().records;
    expect(records).toHaveLength(MAX_AGENT_ARTIFACT_FOLLOW_OWNERS);
    expect(agentArtifactFollowRecordFor(records, ownerA)).toBeNull();
    expect(agentArtifactFollowRecordFor(records, ownerB)?.selected).toEqual({
      kind: "tab",
      tabId: "file:b",
    });
    expect(agentArtifactFollowRecordFor(records, ownerC)?.selected).toEqual({
      kind: "tab",
      tabId: "file:c",
    });
  });

  it("reconciles a closed pinned target to the newest still-visible resource", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.observe(ownerA, { kind: "preview" }, 3);
    store.pin(ownerA, { kind: "tab", tabId: "file:closed" });
    store.reconcile(ownerA, [{ kind: "preview" }]);

    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toMatchObject({
      mode: "follow",
      selected: { kind: "preview" },
      recent: [{ kind: "preview" }],
    });
  });

  it("removes a Preview-only owner when no resource remains available", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.pin(ownerB, { kind: "tab", tabId: "file:b" });
    store.observe(ownerA, { kind: "preview" }, 3);

    store.reconcile(ownerA, []);

    const records = useAgentArtifactFollowStore.getState().records;
    expect(agentArtifactFollowRecordFor(records, ownerA)).toBeNull();
    expect(agentArtifactFollowRecordFor(records, ownerB)?.selected).toEqual({
      kind: "tab",
      tabId: "file:b",
    });
  });

  it("ignores replayed Preview revisions and bounds each target inventory", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.observe(ownerA, { kind: "preview" }, 5);
    store.pin(ownerA, { kind: "tab", tabId: "file:pinned" });
    store.observe(ownerA, { kind: "preview" }, 5);
    const replayRecord = agentArtifactFollowRecordFor(
      useAgentArtifactFollowStore.getState().records,
      ownerA,
    );
    expect(replayRecord).toMatchObject({
      selected: { kind: "tab", tabId: "file:pinned" },
      recent: [{ kind: "preview" }, { kind: "tab", tabId: "file:pinned" }],
      lastPreviewRevision: 5,
    });

    for (let index = 0; index < MAX_AGENT_ARTIFACT_TARGETS + 1; index += 1) {
      store.observe(ownerA, { kind: "tab", tabId: `file:${index}` });
    }
    const record = agentArtifactFollowRecordFor(
      useAgentArtifactFollowStore.getState().records,
      ownerA,
    );
    expect(record?.recent).toHaveLength(MAX_AGENT_ARTIFACT_TARGETS);
    expect(record?.recent[0]).toEqual({ kind: "tab", tabId: "file:1" });
    expect(record?.selected).toEqual({ kind: "tab", tabId: "file:pinned" });

    store.reconcile(ownerA, [{ kind: "tab", tabId: "file:pinned" }]);
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        ownerA,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "tab", tabId: "file:pinned" },
      recent: [{ kind: "tab", tabId: "file:pinned" }],
    });
  });

  it("rejects invalid owners, targets, revisions, and availability lists", () => {
    const store = useAgentArtifactFollowStore.getState();
    store.observe(ownerA, { kind: "tab", tabId: "" });
    store.observe(ownerA, { kind: "preview" }, 0);
    store.observe({ ...ownerA, chatId: "" } as AgentArtifactOwner, {
      kind: "tab",
      tabId: "file:a",
    });
    store.reconcile(ownerA, [
      { kind: "tab", tabId: "" },
      ...Array.from({ length: MAX_AGENT_ARTIFACT_TARGETS + 1 }, (_, index) => ({
        kind: "tab" as const,
        tabId: `file:${index}`,
      })),
    ]);
    expect(useAgentArtifactFollowStore.getState().records).toEqual([]);
  });
});
