import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, AcpItem, TurnProjection } from "../domain/acp";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentTaskHeader } from "./AgentTaskHeader";

const initialState = useWorkbenchStore.getState();
const tab: WorkbenchTab = {
  id: "chat:a",
  kind: "chat",
  title: "Agent View",
  position: 0,
  pinned: false,
  dirty: false,
};

function collaboration(
  id: string,
  ordinal: number,
  operation: string,
  keys: string[],
  labels: string[],
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
      activityId: "activity-1",
      label: operation,
      summary: "",
      receiverLabels: labels,
      operation,
      agentKey: null,
      agentKeys: keys,
      agentStates: states,
    },
  } as unknown as AcpItem;
}

beforeEach(() => {
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState({
    ...initialState,
    project: { id: "project-a", name: "Sage", roots: [] },
    projects: [{ id: "project-a", name: "Sage", roots: [] }],
    tabs: [tab],
    activeTabId: tab.id,
  } as never);
  const store = acpProjectionStoreFor(tab.id);
  const scope = {
    projectId: "project-a",
    projectGeneration: "7",
    chatId: tab.id,
  } as never;
  store.getState().activateScope(scope);
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
    items: [
      collaboration(
        "spawn-1",
        0,
        "spawnAgent",
        ["private-thread-a", "private-thread-b"],
        ["Researcher", "Builder"],
        [
          ["private-thread-a", "running"],
          ["private-thread-b", "running"],
        ],
      ),
      collaboration(
        "wait-1",
        1,
        "wait",
        ["private-thread-b"],
        ["Builder"],
        [["private-thread-b", "completed"]],
      ),
    ],
    activities: [],
    browserOperations: [],
    browserOperationsCollapsed: 0,
  } as unknown as TurnProjection;
  act(() => {
    store.getState().applyAuthoritativePage(scope, {
      projectId: "project-a",
      chatId: tab.id,
      title: "Agent View",
      projectionRevision: "1",
      currentSessionId: "session-1",
      sessions: [{ id: "session-1", runtimeEpoch: "3" }],
      turns: [turn],
      nextCursor: null,
    } as unknown as AcpChatPage);
  });
});

afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialState, true);
});

it("separates recent working and done figures without an inert child-open control", () => {
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  const section = screen.getByRole("region", { name: "Subagents" });
  expect(
    within(section).getByLabelText("Recent subagents working: 1"),
  ).toHaveTextContent("1 working");
  expect(
    within(section).getByLabelText("Recent subagents done: 1"),
  ).toHaveTextContent("1 done");
  expect(
    section.querySelectorAll(".agent-task-header__agent-glyph"),
  ).toHaveLength(1);
  expect(section).not.toHaveTextContent("private-thread");
  expect(within(section).queryByRole("button")).toBeNull();
});
