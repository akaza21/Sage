import {
  sameAgentArtifactTarget,
  agentArtifactFollowRecordFor,
} from "../app/agentArtifactFollowStore";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { seedSnapshotOwner, snapshotRef } from "../app/agentDiff.fixture";
import { useWorkbenchStore } from "../app/store";
import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { AgentToolsContext } from "./AgentToolsProvider";
import { useAgentChangesState } from "./agentChangesState";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import type { AcpChatPage, AcpItem } from "../domain/acp";

const initial = useWorkbenchStore.getState();

it("counts deleted display resources without offering them as file outputs", async () => {
  const projection = seedSnapshotOwner();
  const { page, scope } = projection.getState();
  if (!page || !scope) throw new Error("Missing page");
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Missing changes");
  projection.getState().applyAuthoritativePage(scope, {
    ...page,
    projectionRevision: "2" as AcpChatPage["projectionRevision"],
    turns: [
      {
        ...page.turns[0],
        items: [
          {
            ...item,
            payload: {
              ...item.payload,
              changes: [
                {
                  ...item.payload.changes[0],
                  pairState: undefined,
                  snapshotRevision: undefined,
                  contentDigest: undefined,
                  oldBytes: undefined,
                  newBytes: undefined,
                  operation: "deleted",
                  resource: {
                    kind: "unavailable",
                    reason: "missing",
                    label: "deleted.ts",
                    displayResource: {
                      rootId: "root-a",
                      relativePath: "deleted.ts",
                    },
                  },
                },
              ],
            },
          },
        ],
      },
    ],
  });
  render(
    <AgentTaskHeader
      tab={{
        id: page.chatId,
        kind: "chat",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      }}
      chatCount={1}
      snapshot={null}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(await screen.findByText("1 recent file")).toBeVisible();
  expect(screen.queryByRole("button", { name: /Open.*deleted.ts/ })).toBeNull();
});
afterEach(() => {
  cleanup();
  useAgentChangesState.getState().close();
  useSubagentPaneStore.getState().clear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initial, true);
});

it("disables retained Outputs from an earlier session with an explanation", async () => {
  const projection = seedSnapshotOwner();
  const { page, scope } = projection.getState();
  if (!page || !scope) throw new Error("Missing page");
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Missing changes");
  projection.getState().applyAuthoritativePage(scope, {
    ...page,
    currentSessionId: "next-session" as AcpChatPage["currentSessionId"],
    projectionRevision: "2" as AcpChatPage["projectionRevision"],
    sessions: [
      ...page.sessions,
      {
        ...page.sessions[0],
        id: "next-session" as AcpChatPage["sessions"][number]["id"],
      },
    ],
    turns: [
      {
        ...page.turns[0],
        items: [
          {
            ...item,
            payload: {
              ...item.payload,
              changes: item.payload.changes.map((change) => ({
                ...change,
                operation: "created",
              })),
            },
          } as AcpItem,
        ],
      },
    ],
  } as AcpChatPage);
  render(
    <AgentTaskHeader
      tab={{
        id: snapshotRef.chatId,
        kind: "chat",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      }}
      chatCount={1}
      snapshot={null}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  const output = await screen.findByRole("button", {
    name: "Open output src/a.ts",
  });
  expect(output).toBeDisabled();
  expect(output).toHaveAttribute(
    "title",
    "This output belongs to an earlier session and cannot be opened here.",
  );
});
it("lists edited files through Changes rather than pretending updates are created outputs", async () => {
  seedSnapshotOwner();
  const launch = vi.fn();
  render(
    <AgentToolsContext
      value={{ launch, chatAvailable: true, terminalAvailable: false }}
    >
      <AgentTaskHeader
        tab={{
          id: snapshotRef.chatId,
          kind: "chat",
          title: "Chat",
          position: 0,
          pinned: false,
          dirty: false,
        }}
        chatCount={1}
        snapshot={null}
      />
    </AgentToolsContext>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(await screen.findByText("1 recent file")).toBeVisible();
  expect(
    screen.getByText("Process tracking is not exposed by this session."),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: /^Open output/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Changes" }));
  expect(launch).toHaveBeenCalledWith("changes", snapshotRef.chatId);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("selects an output instead of leaving Changes in front", async () => {
  const projection = seedSnapshotOwner();
  const { page, scope } = projection.getState();
  if (!page || !scope) throw new Error("Missing page");
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Missing changes");
  projection.getState().applyAuthoritativePage(scope, {
    ...page,
    projectionRevision: "2" as AcpChatPage["projectionRevision"],
    turns: [
      {
        ...page.turns[0],
        items: [
          {
            ...item,
            payload: {
              ...item.payload,
              changes: item.payload.changes.map((change) => ({
                ...change,
                operation: "created",
              })),
            },
          } as AcpItem,
        ],
      },
    ],
  });
  useAgentChangesState.getState().show(snapshotRef, 1);
  useSubagentPaneStore
    .getState()
    .show({ owner: snapshotRef, itemId: "child", agentKey: "child" }, true);
  useWorkbenchStore.setState({
    openDiff: async (resource, repository, diffRef) => {
      useWorkbenchStore.setState({
        tabs: [
          {
            id: "output-diff",
            kind: "diff",
            title: resource.relativePath,
            resource,
            repository,
            diffRef,
            position: 0,
            pinned: false,
            dirty: false,
          },
        ],
        activeTabId: "output-diff",
      });
    },
  });
  render(
    <AgentTaskHeader
      tab={{
        id: snapshotRef.chatId,
        kind: "chat",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      }}
      chatCount={1}
      snapshot={null}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Open output src/a.ts" }),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(useWorkbenchStore.getState().activeTabId).toBe("output-diff");
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        snapshotRef,
      )?.selected,
      { kind: "tab", tabId: "output-diff" },
    ),
  ).toBe(true);
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useAgentChangesState.getState().owner,
      )?.selected,
      { kind: "changes" },
    ),
  ).toBe(false);
  expect(
    sameAgentArtifactTarget(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        useSubagentPaneStore.getState().open?.owner,
      )?.selected,
      {
        kind: "subagents",
      },
    ),
  ).toBe(false);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("lists historical browser sources without rebinding them to the current session", async () => {
  const projection = seedSnapshotOwner();
  const { page, scope } = projection.getState();
  if (!page || !scope) throw new Error("Missing page");
  projection.getState().applyAuthoritativePage(scope, {
    ...page,
    projectionRevision: "2" as AcpChatPage["projectionRevision"],
    turns: [
      {
        ...page.turns[0],
        browserOperations: [
          {
            projectId: page.projectId,
            chatId: page.chatId,
            sessionId: page.currentSessionId,
            turnId: page.turns[0].id,
            origin: "https://example.com",
            tabId: "historical-browser",
          },
        ],
      },
    ],
  } as unknown as AcpChatPage);
  const activateTab = vi.fn();
  useWorkbenchStore.setState({ activateTab });
  render(
    <AgentTaskHeader
      tab={{
        id: snapshotRef.chatId,
        kind: "chat",
        title: "Chat",
        position: 0,
        pinned: false,
        dirty: false,
      }}
      chatCount={1}
      snapshot={null}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(await screen.findByText("example.com")).toBeVisible();
  expect(
    screen.getByTitle("example.com - Recorded source; not a live browser link"),
  ).toBeVisible();
  expect(
    screen.queryByRole("button", { name: /source example.com/i }),
  ).toBeNull();
  expect(activateTab).not.toHaveBeenCalled();
});
