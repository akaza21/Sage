import { expect, it } from "vitest";
import type { AcpItem } from "../../../domain/acp";
import { subagentIdentities, withSubagentNames } from "./subagentActivity";

const events = [
  {
    id: "spawn",
    kind: "collaboration",
    payload: {
      operation: "spawnAgent",
      label: "spawnAgent",
      agentKeys: ["a", "b"],
      receiverLabels: ["", ""],
      agentStates: [
        ["a", "completed"],
        ["b", "completed"],
      ],
    },
  },
  {
    id: "native-a",
    kind: "collaboration",
    payload: {
      operation: "completed",
      label: "Reviewer",
      agentKey: "a",
      agentKeys: [],
      receiverLabels: [],
      agentStates: [],
    },
  },
  {
    id: "native-b",
    kind: "collaboration",
    payload: {
      operation: "completed",
      label: "Builder",
      agentKey: "b",
      agentKeys: [],
      receiverLabels: [],
      agentStates: [],
    },
  },
] as unknown as AcpItem[];

it("counts distinct children across grouped and repeated native observations", () => {
  expect([...subagentIdentities(events).values()]).toEqual([
    "Reviewer",
    "Builder",
  ]);
});

it("uses only emitted names, including a later native observation in the turn", () => {
  const items = withSubagentNames(events);
  expect(
    items[0].kind === "collaboration" && items[0].payload.receiverLabels,
  ).toEqual(["Reviewer", "Builder"]);
  const unnamed = withSubagentNames(events.slice(0, 1));
  expect(unnamed[0]).toBe(events[0]);
});
