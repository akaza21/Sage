import { cleanup, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, expect, it } from "vitest";

import type { AcpChatPage, AcpItem, TurnProjection } from "../domain/acp";
import { AgentArtifactCollapsedPane } from "./AgentArtifactCollapsedPane";

afterEach(cleanup);

function collaboration(
  id: string,
  ordinal: number,
  operation: string,
  keys: string[],
  states: [string, string][],
): AcpItem {
  return {
    id,
    projectId: "project-a",
    chatId: "chat:a",
    sessionId: "session-1",
    turnId: "turn-1",
    runtimeEpoch: "3",
    ordinal,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    kind: "collaboration",
    toolState: null,
    payload: {
      activityId: `activity-${id}`,
      label: operation,
      summary: "",
      receiverLabels: keys.map(() => "Reviewer"),
      operation,
      agentKey: null,
      agentKeys: keys,
      agentStates: states,
    },
  } as unknown as AcpItem;
}

function renderPane(items: AcpItem[]) {
  const header = document.createElement("div");
  document.body.append(header);
  const turn = {
    id: "turn-1",
    sessionId: "session-1",
    runtimeEpoch: "3",
    ordinal: 0,
    state: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 2,
    terminalAt: 2,
    items,
    activities: [],
    browserOperations: [],
    browserOperationsCollapsed: 0,
  } as unknown as TurnProjection;
  const page = {
    projectId: "project-a",
    chatId: "chat:a",
    title: "Agent View",
    projectionRevision: "1",
    currentSessionId: "session-1",
    sessions: [{ id: "session-1", runtimeEpoch: "3" }],
    turns: [turn],
    nextCursor: null,
  } as unknown as AcpChatPage;
  render(
    <AgentArtifactCollapsedPane
      count={0}
      subagentPage={page}
      subagentItemId={items[0]!.id}
      edgeTriggerRef={createRef()}
      onExpand={() => {}}
      paneId="pane"
      overlay={false}
      headerNode={header}
    />,
  );
  return screen.getByRole("button");
}

it("counts two spawns that reuse one thread id as two subagents", () => {
  const trigger = renderPane([
    collaboration(
      "spawn-1",
      0,
      "spawnAgent",
      ["thread-a"],
      [["thread-a", "completed"]],
    ),
    collaboration(
      "spawn-2",
      1,
      "spawnAgent",
      ["thread-a"],
      [["thread-a", "running"]],
    ),
  ]);
  expect(trigger).toHaveAccessibleName(
    "Show files and preview · Ready · 2 resources",
  );
});

it("does not count repeated observations of one spawn again", () => {
  const trigger = renderPane([
    collaboration(
      "spawn-1",
      0,
      "spawnAgent",
      ["thread-a"],
      [["thread-a", "running"]],
    ),
    collaboration(
      "wait-1",
      1,
      "wait",
      ["thread-a"],
      [["thread-a", "completed"]],
    ),
  ]);
  expect(trigger).toHaveAccessibleName(
    "Show files and preview · Ready · 1 resource",
  );
});
