import {
  sameAgentArtifactTarget,
  agentArtifactFollowRecordFor,
} from "../app/agentArtifactFollowStore";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { seedSnapshotOwner } from "../app/agentDiff.fixture";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import { settleLazyModules } from "../test/settleLazyModules";
import { AgentChangesObserver } from "./AgentChangesViews";
import { useAgentChangesState } from "./agentChangesState";

const initial = useWorkbenchStore.getState();
afterEach(() => {
  cleanup();
  clearAcpProjectionRegistry();
  useAgentChangesState.getState().close();
  useWorkbenchStore.setState(initial, true);
});

it("leaves historical edits closed and follows a newly observed completion", async () => {
  const projection = seedSnapshotOwner();
  const page = projection.getState().page!;
  const view = render(<AgentChangesObserver page={page} generation={1} />);
  await settleLazyModules();
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useAgentChangesState.getState().owner,
      )?.selected,
      { kind: "changes" },
    ),
  ).toBe(false);
  // A refreshed page with the same terminal identity is still history.
  view.rerender(<AgentChangesObserver page={{ ...page }} generation={1} />);
  await settleLazyModules();
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useAgentChangesState.getState().owner,
      )?.selected,
      { kind: "changes" },
    ),
  ).toBe(false);
  view.rerender(
    <AgentChangesObserver
      page={{ ...page, turns: [{ ...page.turns[0], state: "running" }] }}
      generation={1}
    />,
  );
  await settleLazyModules();
  await act(async () => {
    projection.setState({ page });
    view.rerender(<AgentChangesObserver page={page} generation={1} />);
  });
  await settleLazyModules();
  expect(useAgentChangesState.getState()).toMatchObject({
    generation: 1,
    owner: { projectId: "project-a", chatId: "chat-a", sessionId: "session-a" },
  });
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useAgentChangesState.getState().owner,
      )?.selected,
      { kind: "changes" },
    ),
  ).toBe(true);
});

it("does not follow an older edit when another session's turn completes", async () => {
  const projection = seedSnapshotOwner();
  const page = projection.getState().page!;
  const view = render(<AgentChangesObserver page={page} generation={1} />);
  for (const state of ["running", "completed"] as const) {
    const next = {
      ...page,
      turns: [
        ...page.turns,
        {
          ...page.turns[0],
          id: "other-turn" as never,
          sessionId: "other-session" as never,
          ordinal: 2,
          state,
          items: [],
        },
      ],
    };
    projection.setState({ page: next });
    view.rerender(<AgentChangesObserver page={next} generation={1} />);
    await settleLazyModules();
  }
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useAgentChangesState.getState().owner,
      )?.selected,
      { kind: "changes" },
    ),
  ).toBe(false);
});
