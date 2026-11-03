import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
} from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import type { AcpChatPage, AcpItem, TurnProjection } from "../domain/acp";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { useAgentChangesState } from "./agentChangesState";

const readChild = vi.hoisted(() => vi.fn());
vi.mock("../services/acpChildTranscript", () => ({
  getAcpChildTranscript: readChild,
  subscribeChildProjectionChanges: async () => () => undefined,
}));

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
  readChild.mockClear();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState({
    ...initialState,
    project: { id: "project-a", name: "Sage", roots: [] },
    projects: [{ id: "project-a", name: "Sage", roots: [] }],
    tabs: [tab],
    activeTabId: tab.id,
    projectGeneration: 7,
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

function replaceItems(items: AcpItem[], updates: Partial<AcpChatPage> = {}) {
  const store = acpProjectionStoreFor(tab.id);
  const { scope, page } = store.getState();
  if (!scope || !page) throw new Error("Missing projection");
  act(() => {
    store.getState().applyAuthoritativePage(scope, {
      ...page,
      projectionRevision: String(
        Number(page.projectionRevision) + 1,
      ) as AcpChatPage["projectionRevision"],
      turns: [{ ...page.turns[0], items }],
      ...updates,
    });
  });
}

function sourceItems() {
  return Array.from(
    { length: 4 },
    (_, index) =>
      ({
        ...collaboration(`source-${index}`, index, "started", [], [], []),
        kind: "toolCall",
        toolState: "completed",
        payload: {
          presentation: {
            kind: "fetch",
            label: "Fetched source",
            url: `https://source-${index}.example`,
            summary: null,
          },
          inputSummary: null,
        },
      }) as unknown as AcpItem,
  );
}

it.each(["session", "runtime"])(
  "resets expanded Sources when the %s changes while resources are closed",
  async (replacement) => {
    const items = sourceItems();
    replaceItems(items);
    render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
    const trigger = screen.getByRole("button", { name: "Show chat resources" });
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("button", { name: "View all" }));
    expect(screen.getByText("source-0.example")).toBeVisible();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Chat resources" }), {
      key: "Escape",
    });
    const page = acpProjectionStoreFor(tab.id).getState().page!;
    replaceItems(
      items,
      replacement === "session"
        ? {
            currentSessionId: "session-2" as AcpChatPage["currentSessionId"],
          }
        : {
            sessions: page.sessions.map((session) => ({
              ...session,
              runtimeEpoch: "4" as typeof session.runtimeEpoch,
            })),
          },
    );
    fireEvent.click(trigger);
    expect(
      await screen.findByRole("button", { name: "View all" }),
    ).toBeVisible();
    expect(screen.queryByText("source-0.example")).toBeNull();
  },
);

it("keeps expanded Sources across close and reopen, but resets them on an owner change", async () => {
  const items = sourceItems();
  replaceItems(items);
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  const trigger = screen.getByRole("button", { name: "Show chat resources" });
  fireEvent.click(trigger);
  fireEvent.click(await screen.findByRole("button", { name: "View all" }));
  expect(screen.getByText("source-0.example")).toBeVisible();
  fireEvent.keyDown(screen.getByRole("dialog", { name: "Chat resources" }), {
    key: "Escape",
  });
  fireEvent.click(trigger);
  expect(screen.getByText("source-0.example")).toBeVisible();
  replaceItems(items, {
    currentSessionId: "session-2" as AcpChatPage["currentSessionId"],
  });
  fireEvent.click(trigger);
  expect(screen.queryByText("source-0.example")).toBeNull();
  expect(screen.getByRole("button", { name: "View all" })).toBeVisible();
});

it("counts spawn invocations, not repeated spawn, start, finish, or resource observations", async () => {
  const items = acpProjectionStoreFor(tab.id).getState().page!.turns[0]
    .items as AcpItem[];
  const reobservedSpawn = (revision: string) =>
    ({ ...items[0], revision, updatedAt: Number(revision) }) as AcpItem;
  const observations = (revision: string) => [
    reobservedSpawn(revision),
    items[1],
    collaboration(
      "started-a",
      3,
      "started",
      ["private-thread-a"],
      ["Researcher"],
      [["private-thread-a", "running"]],
    ),
    collaboration(
      "finished-a",
      4,
      "wait",
      ["private-thread-a"],
      ["Researcher"],
      [["private-thread-a", "completed"]],
    ),
    {
      ...items[0],
      id: "source",
      kind: "toolCall",
      toolState: "completed",
      payload: {
        presentation: {
          kind: "fetch",
          label: "Fetched source",
          url: "https://example.com",
        },
      },
    } as unknown as AcpItem,
  ];
  replaceItems(observations("2"));
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(
    await screen.findByLabelText("Recent subagents done: 2"),
  ).toBeVisible();
  expect(
    await screen.findByRole("heading", { name: "Subagents" }),
  ).toBeVisible();
  expect(
    document.querySelector(".agent-task-header__agent-list")?.children,
  ).toHaveLength(2);
  expect(screen.getByLabelText("Recent subagents done: 2")).toBeVisible();
  expect(screen.getByText("example.com")).toBeVisible();
  // A later revision of the same spawn item is a repeated observation.
  replaceItems(observations("3"));
  expect(screen.getByRole("heading", { name: "Subagents" })).toBeVisible();
  expect(
    document.querySelector(".agent-task-header__agent-list")?.children,
  ).toHaveLength(2);
  expect(screen.getByLabelText("Recent subagents done: 2")).toBeVisible();
  // A separate spawn that reuses a child thread id is another subagent.
  replaceItems([
    ...observations("3"),
    collaboration(
      "spawn-again",
      5,
      "spawnAgent",
      ["private-thread-a"],
      ["Researcher"],
      [["private-thread-a", "running"]],
    ),
  ]);
  expect(screen.getByRole("heading", { name: "Subagents" })).toBeVisible();
  expect(
    document.querySelector(".agent-task-header__agent-list")?.children,
  ).toHaveLength(3);
  expect(screen.getByLabelText("Recent subagents working: 1")).toBeVisible();
  expect(screen.getByLabelText("Recent subagents done: 2")).toBeVisible();
});

it("makes a lazily loaded child row keyboard reachable and returns Tab focus", async () => {
  replaceItems([
    collaboration(
      "spawn-reviewer",
      0,
      "spawnAgent",
      ["private-reviewer"],
      ["Reviewer"],
      [["private-reviewer", "running"]],
    ),
  ]);
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  const trigger = screen.getByRole("button", { name: "Show chat resources" });
  trigger.focus();
  fireEvent.click(trigger);
  const row = await screen.findByRole("button", {
    name: "Open subagent Reviewer",
  });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Changes" })).toHaveFocus(),
  );
  row.focus();
  expect(readChild).not.toHaveBeenCalled();
  expect(useSubagentPaneStore.getState().open).toBeNull();
  fireEvent.keyDown(row, { key: "Tab" });
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  expect(trigger).toHaveFocus();
});

it("drops old-session counts and dismisses an open popover on session replacement", () => {
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  const trigger = screen.getByRole("button", { name: "Show chat resources" });
  fireEvent.click(trigger);
  replaceItems(
    acpProjectionStoreFor(tab.id).getState().page!.turns[0].items as AcpItem[],
    {
      currentSessionId: "session-2" as AcpChatPage["currentSessionId"],
    },
  );
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  fireEvent.click(trigger);
  expect(screen.queryByRole("region", { name: "Subagents" })).toBeNull();
});

it("omits unsupported subagent chrome instead of inventing empty counts", () => {
  replaceItems([]);
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(screen.queryByRole("region", { name: "Subagents" })).toBeNull();
  expect(readChild).not.toHaveBeenCalled();
});

it.each([
  ["named", ["Initializer", "Researcher", "Builder", "Closer", "Reviewer"]],
  ["unnamed", ["", "", "", "", ""]],
  ["mixed", ["Initializer", "", "Builder", "", "Reviewer"]],
])(
  "counts %s children across starting, working, done, stopped and failed observations",
  async (_kind, names) => {
    replaceItems(
      ["pendingInit", "running", "completed", "interrupted", "errored"].map(
        (state, index) =>
          collaboration(
            `spawn-${index}`,
            index,
            "spawnAgent",
            [`private-${index}`],
            [names[index]],
            [[`private-${index}`, state]],
          ),
      ),
    );
    render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    const section = await screen.findByRole("region", { name: "Subagents" });
    expect(
      within(section).getByLabelText("Recent subagents working: 2"),
    ).toHaveTextContent("2 working");
    expect(
      within(section).getByLabelText("Recent subagents done: 1"),
    ).toHaveTextContent("1 done");
    for (const state of ["Starting", "Working", "Done", "Stopped", "Failed"]) {
      expect(
        await within(section).findByText(state, { exact: true }),
      ).toBeInTheDocument();
    }
    const list = section.querySelector(".agent-task-header__agent-list")!;
    expect(list.children).toHaveLength(5);
    for (const [index, row] of Array.from(list.children).entries()) {
      expect(row.firstElementChild).toHaveTextContent(
        names[index] || "Subagent",
      );
      if (row.tagName === "BUTTON")
        expect(row).toHaveAccessibleName(
          names[index]
            ? `Open subagent ${names[index]}`
            : `Open subagent Subagent ${index + 1}, ${["starting", "working", "done", "stopped", "failed"][index]}`,
        );
    }
    expect(section).not.toHaveTextContent("private-");
    expect(readChild).not.toHaveBeenCalled();
  },
);

it("bounds 300 child observations to 128 selectable rows and truthful retained counts", async () => {
  replaceItems(
    Array.from({ length: 300 }, (_, index) =>
      collaboration(
        `spawn-${index}`,
        index,
        "spawnAgent",
        [`private-${index}`],
        [index % 2 ? "" : `Agent ${index}`],
        [[`private-${index}`, index % 2 ? "completed" : "running"]],
      ),
    ),
  );
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  const section = await screen.findByRole("region", { name: "Subagents" });
  expect(
    within(section).getByRole("heading", { name: "Subagents" }),
  ).toBeVisible();
  const rows = await within(section).findAllByRole("button");
  expect(rows).toHaveLength(128);
  expect(rows[0]).toHaveAccessibleName("Open subagent Agent 172");
  expect(rows.at(-1)).toHaveAccessibleName("Open subagent Subagent 128, done");
  expect(
    within(section).getByLabelText("Recent subagents working: 64"),
  ).toBeInTheDocument();
  expect(
    within(section).getByLabelText("Recent subagents done: 64"),
  ).toBeInTheDocument();
  expect(
    section.querySelectorAll(".agent-task-header__agent-glyph"),
  ).toHaveLength(3);
  expect(section).toHaveAttribute(
    "title",
    "The latest 128 subagents found in the latest 256 items of the latest 128 turns",
  );
  expect(section).not.toHaveTextContent("private-");
  expect(readChild).not.toHaveBeenCalled();
});

it.each(["project", "generation", "transition"])(
  "dismisses and withholds child resources after a %s owner change",
  (kind) => {
    render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
    const trigger = screen.getByRole("button", { name: "Show chat resources" });
    fireEvent.click(trigger);
    act(() =>
      useWorkbenchStore.setState(
        kind === "project"
          ? { project: { id: "project-b", name: "Other", roots: [] } }
          : kind === "generation"
            ? { projectGeneration: 8 }
            : { projectTransitioning: true },
      ),
    );
    expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
    fireEvent.click(trigger);
    expect(screen.queryByRole("region", { name: "Subagents" })).toBeNull();
    expect(useSubagentPaneStore.getState().open).toBeNull();
  },
);

it.each(["project", "generation", "transition", "session", "runtime", "chat"])(
  "returns focus from inside the resources popover on a %s reset",
  async (kind) => {
    replaceItems([
      collaboration(
        "spawn-reviewer",
        0,
        "spawnAgent",
        ["private-reviewer"],
        ["Reviewer"],
        [["private-reviewer", "running"]],
      ),
    ]);
    const { rerender } = render(
      <AgentTaskHeader tab={tab} chatCount={2} snapshot={null} />,
    );
    const trigger = screen.getByRole("button", { name: "Show chat resources" });
    fireEvent.click(trigger);
    const row = await screen.findByRole("button", {
      name: "Open subagent Reviewer",
    });
    row.focus();
    expect(row).toHaveFocus();
    if (kind === "chat") {
      rerender(
        <AgentTaskHeader
          tab={{ ...tab, id: "chat:b" }}
          chatCount={2}
          snapshot={null}
        />,
      );
    } else if (kind === "session" || kind === "runtime") {
      replaceItems(
        [],
        kind === "session"
          ? { currentSessionId: "session-2" as AcpChatPage["currentSessionId"] }
          : {
              sessions: acpProjectionStoreFor(tab.id)
                .getState()
                .page!.sessions.map((session) => ({
                  ...session,
                  runtimeEpoch: "4" as typeof session.runtimeEpoch,
                })),
            },
      );
    } else {
      act(() =>
        useWorkbenchStore.setState(
          kind === "project"
            ? { project: { id: "project-b", name: "Other", roots: [] } }
            : kind === "generation"
              ? { projectGeneration: 8 }
              : { projectTransitioning: true },
        ),
      );
    }
    expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
    expect(trigger).toHaveFocus();
  },
);

it("does not steal outside focus when a scope reset dismisses resources", async () => {
  render(
    <>
      <AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />
      <button>Outside</button>
    </>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  await screen.findByText("Researcher");
  const outside = screen.getByRole("button", { name: "Outside" });
  outside.focus();
  replaceItems([], {
    currentSessionId: "session-2" as AcpChatPage["currentSessionId"],
  });
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  expect(outside).toHaveFocus();
});

it("resets resource visibility on Chat changes even when returning to the original Chat", () => {
  const { rerender } = render(
    <AgentTaskHeader tab={tab} chatCount={2} snapshot={null} />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  rerender(
    <AgentTaskHeader
      tab={{ ...tab, id: "chat:b" }}
      chatCount={2}
      snapshot={null}
    />,
  );
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  rerender(<AgentTaskHeader tab={tab} chatCount={2} snapshot={null} />);
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
});

it("keeps Task lifecycle metadata readable without a child selection or transcript read", async () => {
  replaceItems(
    ["inProgress", "completed"].map(
      (toolState, index) =>
        ({
          ...collaboration(`task-${index}`, index, "spawnAgent", [], [], []),
          kind: "toolCall",
          toolState,
          payload: {
            presentation: {
              kind: "subagent",
              label: index ? "Review tests" : "Inspect layout",
              activityId: null,
              taskLifecycle: true,
            },
            inputSummary: null,
          },
        }) as AcpItem,
    ),
  );
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  const section = await screen.findByRole("region", { name: "Subagents" });
  expect(
    await within(section).findByText("Inspect layout"),
  ).toBeInTheDocument();
  expect(await within(section).findByText("Review tests")).toBeInTheDocument();
  expect(within(section).queryByRole("button")).toBeNull();
  expect(section).toHaveTextContent("1 working");
  expect(section).toHaveTextContent("1 done");
  expect(useSubagentPaneStore.getState().open).toBeNull();
  expect(readChild).not.toHaveBeenCalled();
});

afterEach(() => {
  cleanup();
  useSubagentPaneStore.getState().clear();
  useAgentChangesState.getState().close();
  clearAcpProjectionRegistry();
  useWorkbenchStore.setState(initialState, true);
});

it("opens the observed child only after explicit resource selection", async () => {
  useAgentChangesState
    .getState()
    .show(
      { projectId: "project-a", chatId: tab.id, sessionId: "session-1" },
      7,
    );
  const store = acpProjectionStoreFor(tab.id);
  const { scope, page } = store.getState();
  if (!scope || !page) throw new Error("Missing projection");
  act(() => {
    store.getState().applyAuthoritativePage(scope, {
      ...page,
      projectionRevision: "2" as AcpChatPage["projectionRevision"],
      turns: [
        {
          ...page.turns[0],
          items: [
            collaboration(
              "spawn-reviewer",
              0,
              "spawnAgent",
              ["private-reviewer"],
              ["Reviewer"],
              [["private-reviewer", "running"]],
            ),
          ],
        },
      ],
    });
  });
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  expect(useSubagentPaneStore.getState().open).toBeNull();
  const trigger = screen.getByRole("button", { name: "Show chat resources" });
  fireEvent.click(trigger);
  const row = await screen.findByRole("button", {
    name: "Open subagent Reviewer",
  });
  expect(useSubagentPaneStore.getState().open).toBeNull();
  fireEvent.click(row);
  expect(useSubagentPaneStore.getState().open).toEqual({
    owner: { projectId: "project-a", chatId: tab.id, sessionId: "session-1" },
    itemId: "spawn-reviewer",
    agentKey: "private-reviewer",
  });
  expect(useSubagentPaneStore.getState().selected).toBe(true);
  expect(useAgentChangesState.getState().selected).toBe(false);
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  expect(trigger).toHaveFocus();
});

it("selects the bound native activity for a child already completed at spawn", async () => {
  const native = collaboration("native-child", 1, "completed", [], [], []);
  if (native.kind !== "collaboration")
    throw new Error("Expected collaboration");
  replaceItems([
    collaboration(
      "spawn-child",
      0,
      "spawnAgent",
      ["private-child"],
      ["Reviewer"],
      [["private-child", "completed"]],
    ),
    { ...native, payload: { ...native.payload, agentKey: "private-child" } },
  ]);
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(screen.getByLabelText("Recent subagents done: 1")).toBeInTheDocument();
  fireEvent.click(
    await screen.findByRole("button", { name: "Open subagent Reviewer" }),
  );
  expect(useSubagentPaneStore.getState().open?.itemId).toBe("native-child");
});

it("moves a retained running child to done on its native terminal observation", () => {
  const native = collaboration("native-child", 1, "completed", [], [], []);
  if (native.kind !== "collaboration")
    throw new Error("Expected collaboration");
  replaceItems([
    collaboration(
      "spawn-child",
      0,
      "spawnAgent",
      ["private-child"],
      ["Reviewer"],
      [["private-child", "running"]],
    ),
    { ...native, payload: { ...native.payload, agentKey: "private-child" } },
  ]);
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(
    screen.getByLabelText("Recent subagents working: 0"),
  ).toBeInTheDocument();
  expect(screen.getByLabelText("Recent subagents done: 1")).toBeInTheDocument();
});

it.each([
  ["interrupted", "Stopped"],
  ["errored", "Failed"],
])(
  "preserves %s child state after native completion",
  async (state, status) => {
    const native = collaboration("native-child", 2, "completed", [], [], []);
    if (native.kind !== "collaboration")
      throw new Error("Expected collaboration");
    replaceItems([
      collaboration(
        "spawn-child",
        0,
        "spawnAgent",
        ["private-child"],
        ["Reviewer"],
        [["private-child", "running"]],
      ),
      collaboration(
        "stop-child",
        1,
        state === "interrupted" ? "interrupted" : "wait",
        ["private-child"],
        ["Reviewer"],
        [["private-child", state]],
      ),
      { ...native, payload: { ...native.payload, agentKey: "private-child" } },
    ]);
    render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    const row = await screen.findByRole("button", {
      name: "Open subagent Reviewer",
    });
    expect(row).toHaveTextContent(status);
    expect(
      screen.getByLabelText("Recent subagents working: 0"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("Recent subagents done: 0"),
    ).toBeInTheDocument();
  },
);

it("uses a supplied native child name when its retained spawn was unnamed", async () => {
  const native = collaboration("native-child", 1, "started", [], [], []);
  if (native.kind !== "collaboration")
    throw new Error("Expected collaboration");
  replaceItems([
    collaboration(
      "spawn-child",
      0,
      "spawnAgent",
      ["private-child"],
      [""],
      [["private-child", "running"]],
    ),
    {
      ...native,
      payload: {
        ...native.payload,
        label: "Researcher",
        agentKey: "private-child",
      },
    },
  ]);
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  expect(
    await screen.findByRole("button", { name: "Open subagent Researcher" }),
  ).toBeInTheDocument();
});

it("exposes the full project name when its breadcrumb is ellipsized", () => {
  const name = "long-project-name-".repeat(12);
  useWorkbenchStore.setState({ project: { id: "project-a", name, roots: [] } });
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  const crumb = screen.getByText(name);
  expect(crumb).toHaveAttribute("title", name);
});

it("separates recent working and done figures without an inert child-open control", async () => {
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  fireEvent.click(screen.getByRole("button", { name: "Show chat resources" }));
  const section = await screen.findByRole("region", { name: "Subagents" });
  // A plain "Subagents" heading, then the count row; "1 working" once (P1).
  expect(
    within(section).getByRole("heading", { name: "Subagents" }),
  ).toHaveTextContent(/^Subagents$/);
  expect(within(section).getAllByText(/1 working/)).toHaveLength(1);
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
  expect(section).not.toHaveTextContent("not live");
});

it("portals chat resources outside the header and returns Escape focus to the trigger", () => {
  render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
  const trigger = screen.getByRole("button", { name: "Show chat resources" });
  fireEvent.click(trigger);
  const panel = screen.getByRole("dialog", { name: "Chat resources" });
  expect(panel.parentElement).toBe(document.body);
  expect(panel).toHaveClass("menu-popover");
  fireEvent.keyDown(panel, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
  expect(trigger).toHaveFocus();
});

it.each([false, true])(
  "returns resources-dialog Tab focus to its opener (reverse=%s)",
  async (shiftKey) => {
    render(<AgentTaskHeader tab={tab} chatCount={1} snapshot={null} />);
    const trigger = screen.getByRole("button", { name: "Show chat resources" });
    fireEvent.click(trigger);
    const panel = await screen.findByRole("dialog", { name: "Chat resources" });
    expect(
      within(panel).getByRole("button", { name: "Changes" }),
    ).toHaveFocus();
    fireEvent.keyDown(panel, { key: "Tab", shiftKey });
    expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
    expect(trigger).toHaveFocus();
  },
);
