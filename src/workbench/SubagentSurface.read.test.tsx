import {
  act,
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type {
  AcpChatPage,
  ProjectionRevision,
  RuntimeEpoch,
  SageAgentSessionId,
} from "../domain/acp";
import { SubagentSurface } from "./SubagentSurface";

const readChild = vi.hoisted(() => vi.fn());
const desktop = vi.hoisted(() => ({ runtime: false }));
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: readChild,
  isTauriRuntime: () => desktop.runtime,
}));
const childNotices = vi.hoisted(
  () => new Map<string, (epoch: number) => void>(),
);
// Holds the child-notice subscription open until a test releases it.
const subscribeGate = vi.hoisted(() => ({
  wait: null as Promise<void> | null,
}));
vi.mock("../services/acpChildTranscript", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  subscribeChildProjectionChanges: async (
    projectId: string,
    chatId: string,
    onEpoch: (epoch: number) => void,
  ) => {
    if (subscribeGate.wait) await subscribeGate.wait;
    childNotices.set(projectId + "/" + chatId, onEpoch);
    return () => childNotices.delete(projectId + "/" + chatId);
  },
}));
afterEach(() => {
  cleanup();
  readChild.mockReset();
  childNotices.clear();
  subscribeGate.wait = null;
  desktop.runtime = false;
});

function childPage(): AcpChatPage {
  return {
    projectId: "project",
    chatId: "chat",
    projectionRevision: "7",
    currentSessionId: "session",
    sessions: [
      {
        id: "session",
        runtimeEpoch: "2",
        capabilities: { childTranscriptRead: true },
      },
    ],
    turns: [
      {
        id: "turn",
        sessionId: "session",
        runtimeEpoch: "2",
        items: [
          {
            id: "spawn",
            kind: "collaboration",
            payload: {
              agentKey: "child",
              receiverLabels: [],
              agentStates: [],
            },
          },
        ],
        activities: [],
      },
    ],
  } as unknown as AcpChatPage;
}

const current = {
  kind: "current",
  snapshot: {
    childThreadId: "child",
    parentThreadId: "parent",
    retentionLimited: false,
    turns: [],
  },
};

it.each([
  [
    "stale revision",
    { kind: "conflict", resource: "projectionRevision" },
    "Child transcript read is stale. Reopen this view after the chat updates.",
  ],
  [
    "stale owner",
    { kind: "conflict", resource: "session" },
    "Child transcript read is stale. Reopen this view after the chat updates.",
  ],
  [
    "rejected",
    { kind: "rejected" },
    "Child transcript read was rejected. No transcript was loaded.",
  ],
  [
    "unavailable",
    { kind: "unavailable" },
    "Child transcript is unavailable. Try reopening this view.",
  ],
  [
    "recovery required",
    { kind: "recoveryRequired" },
    "Child transcript read needs recovery. Reload the session.",
  ],
  [
    "unknown",
    "private wire details",
    "Child transcript is unavailable. Try reopening this view.",
  ],
])("shows a bounded truthful %s failure", async (_name, failure, message) => {
  readChild.mockRejectedValue(
    typeof failure === "object"
      ? { ...failure, message: "private wire details" }
      : failure,
  );
  render(
    <SubagentSurface
      page={childPage()}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText(message)).toBeVisible();
  expect(screen.queryByText("private wire details")).toBeNull();
  expect(screen.queryByText("No child activity was captured.")).toBeNull();
  expect(readChild).toHaveBeenCalledTimes(1);
});

it("shows too-large without claiming an empty or partial transcript", async () => {
  readChild.mockResolvedValue({ kind: "tooLarge" });
  render(
    <SubagentSurface
      page={childPage()}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(
    await screen.findByText("Child transcript is too large to display."),
  ).toBeVisible();
  expect(screen.queryByText("No child activity was captured.")).toBeNull();
});

it.each(["capability", "epoch", "child", "session"])(
  "never dispatches with missing or mismatched %s",
  (guard) => {
    const page = childPage();
    if (guard === "capability")
      page.sessions![0].capabilities!.childTranscriptRead = false;
    if (guard === "epoch") page.sessions![0].runtimeEpoch = "9" as RuntimeEpoch;
    if (guard === "session")
      page.currentSessionId = "other" as SageAgentSessionId;
    render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey={guard === "child" ? "foreign" : "child"}
      />,
    );
    expect(readChild).not.toHaveBeenCalled();
  },
);

it.each(["project", "chat", "generation", "epoch", "session"])(
  "discards late success and failure after %s changes",
  async (ownerPart) => {
    let finish!: (value: unknown) => void;
    let reject!: (error: unknown) => void;
    readChild.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    readChild.mockImplementationOnce(
      () =>
        new Promise((_resolve, fail) => {
          reject = fail;
        }),
    );
    readChild.mockResolvedValueOnce({ kind: "tooLarge" });
    const page = childPage();
    const view = render(
      <SubagentSurface
        page={page}
        projectGeneration="3"
        itemId="spawn"
        agentKey="child"
      />,
    );
    const changed = childPage();
    if (ownerPart === "project")
      changed.projectId = "other" as typeof changed.projectId;
    if (ownerPart === "chat") changed.chatId = "other" as typeof changed.chatId;
    if (ownerPart === "revision")
      changed.projectionRevision = "8" as ProjectionRevision;
    if (ownerPart === "epoch") {
      changed.sessions![0].runtimeEpoch = "9" as RuntimeEpoch;
      changed.turns[0].runtimeEpoch = "9" as RuntimeEpoch;
    }
    if (ownerPart === "session") {
      changed.currentSessionId = "other" as SageAgentSessionId;
      changed.sessions![0].id = "other" as SageAgentSessionId;
      changed.turns[0].sessionId = "other" as SageAgentSessionId;
    }
    const generation = ownerPart === "generation" ? "4" : "3";
    view.rerender(
      <SubagentSurface
        page={changed}
        projectGeneration={generation}
        itemId="spawn"
        agentKey="child"
      />,
    );
    await act(async () => finish(current));
    expect(screen.queryByText("No child activity was captured.")).toBeNull();
    // The changed owner reads at once, as soon as the stale read settles.
    await waitFor(() => expect(readChild).toHaveBeenCalledTimes(2));
    view.rerender(
      <SubagentSurface
        page={{ ...changed, childProjectionEpoch: 10 }}
        projectGeneration={generation}
        itemId="spawn"
        agentKey="child"
      />,
    );
    await act(async () => reject({ kind: "rejected" }));
    // The same child refreshes after its epoch is quiet, never beside the
    // read already in flight.
    expect(
      await screen.findByText(/too large to display/, undefined, {
        timeout: 3_000,
      }),
    ).toBeVisible();
    expect(readChild).toHaveBeenCalledTimes(3);
  },
);

it("drops a pending read when the Session is superseded", async () => {
  let finish!: (value: unknown) => void;
  readChild.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const page = childPage();
  const view = render(
    <SubagentSurface
      page={page}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  view.rerender(
    <SubagentSurface
      page={{
        ...page,
        currentSessionId: "other" as typeof page.currentSessionId,
      }}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  await act(async () => finish(current));
  await waitFor(() =>
    expect(screen.queryByText("No child activity was captured.")).toBeNull(),
  );
  expect(
    screen.getByText(/earlier session and isn’t available here/),
  ).toBeVisible();
  expect(readChild).toHaveBeenCalledTimes(1);
});

function spawnPage(state: string, revision = "7"): AcpChatPage {
  const page = childPage();
  return {
    ...page,
    projectionRevision: revision as ProjectionRevision,
    childProjectionEpoch: Number(revision),
    sessionConfig: {
      configOptions: [
        {
          id: "model",
          name: "Model",
          category: "model",
          type: "select",
          currentValue: "gpt-6-astra",
          options: [{ value: "gpt-6-astra", name: "GPT-6 Astra" }],
        },
        {
          id: "effort",
          name: "Effort",
          category: "thought_level",
          type: "select",
          currentValue: "medium",
          options: [{ value: "medium", name: "Medium" }],
        },
      ],
    },
    turns: [
      {
        ...page.turns[0],
        items: [
          {
            id: "spawn",
            kind: "collaboration",
            createdAt: 1_000,
            updatedAt: 26_000,
            payload: {
              operation: "spawnAgent",
              agentKeys: ["child"],
              agentStates: [["child", state]],
              receiverLabels: ["Count readme"],
            },
          },
        ],
      },
    ],
  } as unknown as AcpChatPage;
}

const text = (id: string, value: string, role = "assistant") => ({
  id,
  role,
  lifecycle: "completed",
  tool_state: null,
  payload: { kind: "text", payload: { text: value } },
});

function childRead(
  turns: { status: string; items: unknown[] }[],
  spawnModel?: unknown,
) {
  return {
    kind: "current",
    ...(spawnModel ? { spawnModel } : {}),
    snapshot: {
      childThreadId: "child",
      parentThreadId: "parent",
      retentionLimited: false,
      turns: turns.map((turn, index) => ({
        id: `child-turn-${index}`,
        ...turn,
      })),
    },
  };
}

it("refreshes an open live child on each invalidation epoch without losing or duplicating rows", async () => {
  let finish!: (value: unknown) => void;
  readChild.mockResolvedValueOnce(
    childRead([
      {
        status: "inProgress",
        items: [
          text("prompt", "Count the README lines", "user"),
          text("a", "Step A"),
        ],
      },
    ]),
  );
  readChild.mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  const view = render(
    <SubagentSurface
      page={spawnPage("running")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText("Step A")).toBeVisible();
  expect(screen.queryByText("Count the README lines")).toBeNull();
  view.rerender(
    <SubagentSurface
      page={spawnPage("running", "8")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  await waitFor(() => expect(readChild).toHaveBeenCalledTimes(2), {
    timeout: 3_000,
  });
  // Child reads carry no projection revision: they key off the child epoch.
  expect(readChild.mock.lastCall?.[1].request).not.toHaveProperty(
    "expectedProjectionRevision",
  );
  expect(screen.getByText("Step A")).toBeVisible();
  expect(screen.queryByText(/Loading child transcript/)).toBeNull();
  await act(async () =>
    finish(
      childRead([
        {
          status: "completed",
          items: [text("a", "Step A"), text("b", "Step B")],
        },
        { status: "inProgress", items: [text("b", "Step B")] },
      ]),
    ),
  );
  expect(screen.getAllByText("Step B")).toHaveLength(1);
  expect(screen.getByText("Step A")).toBeVisible();
});

it("keeps a finished child readable and stops refreshing once it settles", async () => {
  readChild.mockResolvedValueOnce(
    childRead([
      { status: "completed", items: [text("a", "Both counts done")] },
    ]),
  );
  readChild.mockRejectedValue({
    kind: "conflict",
    resource: "projectionRevision",
  });
  const view = render(
    <SubagentSurface
      page={spawnPage("completed")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText("Both counts done")).toBeVisible();
  view.rerender(
    <SubagentSurface
      page={spawnPage("completed", "9")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  await new Promise((resolve) => setTimeout(resolve, 1_200));
  expect(readChild).toHaveBeenCalledTimes(1);
  expect(screen.getByText("Both counts done")).toBeVisible();
  expect(screen.getByText("Worked for 25s")).toBeVisible();
});

it("offers Copy under a settled child's answer and only for that answer (s4/f0485)", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  readChild.mockResolvedValueOnce(
    childRead([
      {
        status: "completed",
        items: [
          text("a", "Counting first"),
          text("b", "README.md has 2 lines."),
        ],
      },
    ]),
  );
  render(
    <SubagentSurface
      page={spawnPage("completed")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText("README.md has 2 lines.")).toBeVisible();
  const actions = screen.getAllByRole("group", { name: "Message actions" });
  expect(actions).toHaveLength(1);
  await act(async () => {
    within(actions[0]).getByRole("button", { name: "Copy" }).click();
  });
  expect(writeText).toHaveBeenCalledWith("README.md has 2 lines.");
  // Codex child answers offer no "…" menu (spec section 7).
  expect(screen.queryByRole("button", { name: "More actions" })).toBeNull();
  vi.unstubAllGlobals();
});

it("offers no Copy while the child is still working", async () => {
  readChild.mockResolvedValueOnce(
    childRead([{ status: "inProgress", items: [text("a", "Still counting")] }]),
  );
  render(
    <SubagentSurface
      page={spawnPage("running")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText("Still counting")).toBeVisible();
  expect(screen.queryByRole("group", { name: "Message actions" })).toBeNull();
});

it.each([
  [256, false, true],
  [257, false, false],
  [2, true, false],
] as const)(
  "only copies the actual final child row when all %i rows are retained (%s)",
  async (count, retentionLimited, copyable) => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    try {
      const result = childRead([
        {
          status: "completed",
          items: Array.from({ length: count }, (_, index) =>
            text(`answer-${index}`, `Answer ${index}`),
          ),
        },
      ]);
      result.snapshot.retentionLimited = retentionLimited;
      readChild.mockResolvedValueOnce(result);
      render(
        <SubagentSurface
          page={spawnPage("completed")}
          projectGeneration="3"
          itemId="spawn"
          agentKey="child"
        />,
      );
      expect(await screen.findByText("Answer 0")).toBeVisible();
      const copy = screen.queryByRole("button", { name: "Copy" });
      if (copyable) {
        expect(copy).not.toBeNull();
        await act(async () => copy!.click());
        expect(writeText).toHaveBeenCalledWith("Answer 255");
      } else expect(copy).toBeNull();
    } finally {
      vi.unstubAllGlobals();
    }
  },
);

it("keeps the last full read when a later refresh is stale", async () => {
  readChild.mockResolvedValueOnce(
    childRead([{ status: "inProgress", items: [text("a", "Still counting")] }]),
  );
  readChild.mockRejectedValue({
    kind: "conflict",
    resource: "projectionRevision",
  });
  const view = render(
    <SubagentSurface
      page={spawnPage("running")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  expect(await screen.findByText("Still counting")).toBeVisible();
  view.rerender(
    <SubagentSurface
      page={spawnPage("running", "8")}
      projectGeneration="3"
      itemId="spawn"
      agentKey="child"
    />,
  );
  await waitFor(() => expect(readChild).toHaveBeenCalledTimes(2), {
    timeout: 3_000,
  });
  await act(async () => undefined);
  expect(screen.getByText("Still counting")).toBeVisible();
  expect(screen.queryByText(/read is stale/)).toBeNull();
});

const live = (epoch: string) => (
  <SubagentSurface
    page={spawnPage("running", epoch)}
    projectGeneration="3"
    itemId="spawn"
    agentKey="child"
  />
);
const sleep = (ms: number) =>
  act(() => new Promise((resolve) => setTimeout(resolve, ms)));

it("continuous child activity reads about once a second, one read at a time", async () => {
  let inFlight = 0;
  let overlapped = false;
  const starts: number[] = [];
  readChild.mockImplementation(async () => {
    starts.push(performance.now());
    inFlight += 1;
    overlapped ||= inFlight > 1;
    await new Promise((resolve) => setTimeout(resolve, 150));
    inFlight -= 1;
    return childRead([
      { status: "inProgress", items: [text("a", `Step ${starts.length}`)] },
    ]);
  });
  const view = render(live("7"));
  expect(await screen.findByText("Step 1")).toBeVisible();
  // A new epoch every 200 ms for 3.6 s never lets the child go quiet.
  for (let epoch = 8; epoch < 26; epoch += 1) {
    view.rerender(live(String(epoch)));
    await sleep(200);
  }
  // A trailing debounce would read once at the end; the 1 s max wait reads
  // throughout, never twice in one window and never two at once.
  expect(starts.length).toBeGreaterThanOrEqual(4);
  expect(starts.length).toBeLessThanOrEqual(6);
  for (let index = 1; index < starts.length; index += 1)
    expect(starts[index] - starts[index - 1]).toBeGreaterThanOrEqual(950);
  expect(overlapped).toBe(false);
  // Once activity stops, one trailing read settles the last epoch.
  const before = starts.length;
  await sleep(1_300);
  expect(starts.length).toBeLessThanOrEqual(before + 1);
  await sleep(1_200);
  expect(starts.length).toBeLessThanOrEqual(before + 1);
}, 15_000);

it("a fast failure after a slow good read never discards it", async () => {
  let finish!: (value: unknown) => void;
  readChild.mockImplementationOnce(
    () => new Promise((resolve) => (finish = resolve)),
  );
  readChild.mockRejectedValue({ kind: "rejected" });
  const view = render(live("7"));
  view.rerender(live("8"));
  await sleep(1_200);
  // The failing refresh waits for the slow read instead of racing it.
  expect(readChild).toHaveBeenCalledTimes(1);
  await act(async () =>
    finish(childRead([{ status: "inProgress", items: [text("a", "Slow")] }])),
  );
  expect(screen.getByText("Slow")).toBeVisible();
  await waitFor(() => expect(readChild).toHaveBeenCalledTimes(2), {
    timeout: 2_000,
  });
  await act(async () => undefined);
  expect(screen.getByText("Slow")).toBeVisible();
  expect(screen.queryByText(/read was rejected/)).toBeNull();
}, 10_000);

it.each([
  [{ model: "gpt-6-astra", reasoningEffort: "medium" }, "GPT-6 Astra · Medium"],
  [{ model: "unlisted-model" }, "unlisted-model"],
  [undefined, null],
  [{ model: "bad\nmodel" }, null],
])(
  "labels the child header with only the wire's model %#",
  async (spawnModel, label) => {
    readChild.mockResolvedValue(
      childRead(
        [{ status: "completed", items: [text("a", "Done")] }],
        spawnModel,
      ),
    );
    const { container } = render(
      <SubagentSurface
        page={spawnPage("completed")}
        projectGeneration="3"
        itemId="spawn"
        agentKey="child"
      />,
    );
    expect(await screen.findByText("Done")).toBeVisible();
    expect(
      screen.getByRole("heading", { level: 1, name: "Count readme" }),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Back to subagents" }),
    ).toBeVisible();
    const model = container.querySelector(".subagent-surface__model");
    if (label) expect(model).toHaveTextContent(label);
    else expect(model).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  },
);

it("a child-only notice re-reads the open child without a parent page change", async () => {
  readChild.mockResolvedValueOnce(
    childRead([{ status: "inProgress", items: [text("a", "Step A")] }]),
  );
  readChild.mockResolvedValue(
    childRead([{ status: "inProgress", items: [text("b", "Step B")] }]),
  );
  render(live("7"));
  expect(await screen.findByText("Step A")).toBeVisible();
  await waitFor(() => expect(childNotices.has("project/chat")).toBe(true));
  // The same page object stays mounted; only the child notice arrives.
  act(() => childNotices.get("project/chat")!(1_000));
  expect(
    await screen.findByText("Step B", undefined, { timeout: 2_000 }),
  ).toBeVisible();
  expect(readChild).toHaveBeenCalledTimes(2);
}, 10_000);

it("the first child read waits until child notices are subscribed", async () => {
  let open!: () => void;
  desktop.runtime = true;
  subscribeGate.wait = new Promise((resolve) => (open = resolve));
  readChild.mockResolvedValue(
    childRead([{ status: "inProgress", items: [text("a", "Step A")] }]),
  );
  render(live("7"));
  await act(async () => {});
  // A notice published before the listener exists would be lost, so no
  // read starts until the subscription is attached.
  expect(readChild).not.toHaveBeenCalled();
  await act(async () => open());
  expect(await screen.findByText("Step A")).toBeVisible();
  expect(childNotices.has("project/chat")).toBe(true);
  expect(readChild).toHaveBeenCalledTimes(1);
});
