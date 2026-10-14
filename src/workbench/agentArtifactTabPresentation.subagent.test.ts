import { expect, it } from "vitest";

import type { AcpChatPage, AcpItem } from "../domain/acp";
import { projectTaskResources } from "./agentArtifactTabPresentation";

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
    { items: [spawn], statuses: ["starting", "starting"], working: 2, done: 0 },
    {
      items: [spawn, started],
      statuses: ["working", "starting"],
      working: 2,
      done: 0,
    },
    {
      items: [spawn, started, done],
      statuses: ["done", "starting"],
      working: 1,
      done: 1,
    },
    {
      items: [spawn, started, done, stopped],
      statuses: ["done", "stopped"],
      working: 0,
      done: 1,
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
    expect(result.workingAgents).toBe(stage.working);
    expect(result.doneAgents).toBe(stage.done);
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
  expect(result.workingAgents).toBe(0);
  expect(result.doneAgents).toBe(1);
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
    },
  ]);
  expect(result.workingAgents).toBe(0);
  expect(result.doneAgents).toBe(1);
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
    expect(result.workingAgents).toBe(0);
    expect(result.doneAgents).toBe(1);
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
