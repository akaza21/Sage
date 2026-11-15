import { expect, it } from "vitest";

import type { AcpChatPage, AcpItem } from "../domain/acp";
import { projectTaskResources } from "./agentArtifactTabPresentation";
import { unconfirmedTurn } from "./subagentLiveness";

function event(
  id: string,
  operation: string,
  keys: string[],
  labels: string[],
  states: [string, string][],
  agentKey: string | null = null,
): AcpItem {
  return {
    id,
    kind: "collaboration",
    payload: {
      operation,
      agentKey,
      agentKeys: keys,
      receiverLabels: labels,
      agentStates: states,
    },
  } as unknown as AcpItem;
}

function summary(items: AcpItem[]) {
  const page = {
    projectId: "project",
    chatId: "chat",
    turns: [
      {
        id: "turn",
        sessionId: "session",
        items,
        browserOperations: [],
      },
    ],
  } as unknown as AcpChatPage;
  return projectTaskResources(page, "chat");
}

it("reconciles each admitted child without duplicating counts or shuffling rows", () => {
  const spawn = event(
    "spawn",
    "spawnAgent",
    ["a", "b"],
    ["Researcher", "Builder"],
    [
      ["a", "pendingInit"],
      ["b", "pendingInit"],
    ],
  );
  const started = event(
    "start-a",
    "started",
    ["a"],
    ["Researcher"],
    [["a", "running"]],
    "a",
  );
  const done = event(
    "wait-a",
    "wait",
    ["a"],
    ["Researcher"],
    [["a", "completed"]],
  );
  const stopped = event(
    "stop-b",
    "interrupted",
    ["b"],
    ["Builder"],
    [["b", "interrupted"]],
    "b",
  );
  const stages = [
    { items: [spawn], statuses: ["starting", "starting"] },
    {
      items: [spawn, started],
      statuses: ["working", "starting"],
    },
    {
      items: [spawn, started, done],
      statuses: ["done", "starting"],
    },
    {
      items: [spawn, started, done, stopped],
      statuses: ["done", "stopped"],
    },
  ];
  for (const stage of stages) {
    const result = summary(stage.items);
    expect(result.recentAgents.map((agent) => agent.label)).toEqual([
      "Researcher",
      "Builder",
    ]);
    expect(result.recentAgents.map((agent) => agent.status)).toEqual(
      stage.statuses,
    );
  }
});

it("retains a failed child beside independently completed siblings", () => {
  const result = summary([
    event(
      "spawn",
      "spawnAgent",
      ["a", "b"],
      ["Researcher", "Builder"],
      [
        ["a", "running"],
        ["b", "running"],
      ],
    ),
    event(
      "wait",
      "wait",
      ["a", "b"],
      ["Researcher", "Builder"],
      [
        ["a", "completed"],
        ["b", "errored"],
      ],
    ),
  ]);
  expect(result.recentAgents.map((agent) => agent.status)).toEqual([
    "done",
    "failed",
  ]);
});

it("counts paired spawn and native completion observations as one child", () => {
  const result = summary([
    event(
      "spawn",
      "spawnAgent",
      ["child-1"],
      ["Reviewer"],
      [["child-1", "completed"]],
    ),
    event("activity", "completed", [], [], [], "child-1"),
  ]);
  expect(result.recentAgents).toEqual([
    {
      id: "spawn:child-1",
      label: "Reviewer",
      status: "done",
      agentKey: "child-1",
      reportedTurn: expect.objectContaining({ id: "turn" }),
    },
  ]);
});

it.each(["pendingInit", "running"])(
  "completes a %s child even when its selection target is unavailable",
  (state) => {
    const result = summary([
      event("spawn", "spawnAgent", ["a"], ["Reviewer"], [["a", state]]),
      event("native", "completed", [], [], [], "a"),
    ]);
    expect(result.recentAgents[0]).toMatchObject({ status: "done" });
    expect(result.recentAgents[0].target).toBeUndefined();
  },
);

it("does not infer a child from a started event without a retained spawn", () => {
  const started = event(
    "started",
    "started",
    ["a"],
    ["Researcher"],
    [["a", "running"]],
    "a",
  );
  const done = event(
    "wait",
    "wait",
    ["a"],
    ["Researcher"],
    [["a", "completed"]],
  );
  expect(summary([started]).recentAgents).toEqual([]);
  expect(
    summary([started, done]).recentAgents.map((agent) => agent.status),
  ).toEqual([]);
});

it("leaves a live child unconfirmed only while its latest parent row sits in an ended turn", () => {
  const turn = (id: string, state: string, items: AcpItem[]) => ({
    id,
    state,
    sessionId: "session",
    items,
    browserOperations: [],
  });
  const recent = (turns: ReturnType<typeof turn>[]) =>
    projectTaskResources(
      { projectId: "project", chatId: "chat", turns } as unknown as AcpChatPage,
      "chat",
    ).recentAgents.map((agent) => ({
      status: agent.status,
      unconfirmed: unconfirmedTurn(agent)?.id,
    }));
  const spawn = (key: string, state: string) =>
    event(`spawn-${key}`, "spawnAgent", [key], ["Reviewer"], [[key, state]]);
  const ended = turn("first", "completed", [
    spawn("a", "running"),
    spawn("b", "completed"),
  ]);
  expect(recent([ended])).toEqual([
    { status: "working", unconfirmed: "first" },
    { status: "done", unconfirmed: undefined },
  ]);
  expect(recent([{ ...ended, state: "running" }])[0].unconfirmed).toBe(
    undefined,
  );
  // A later report about the child in a live turn can still change it.
  const reported = turn("second", "running", [
    event("wait-a", "wait", ["a"], ["Reviewer"], [["a", "running"]]),
  ]);
  expect(recent([ended, reported])[0]).toEqual({
    status: "working",
    unconfirmed: undefined,
  });
});
