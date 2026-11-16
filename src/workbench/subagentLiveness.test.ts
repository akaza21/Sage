import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import type { AcpChatPage } from "../domain/acp";
import { projectRecentAgents } from "./agentArtifactTabPresentation";
import { resetSubagentLiveness, useSettledSubagents } from "./subagentLiveness";

const transcripts = vi.hoisted(
  () => new Map<string, { kind: string; status: string }>(),
);
const reads = vi.hoisted(() => [] as string[]);
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  isTauriRuntime: () => false,
  invokeNative: async (
    _command: string,
    { request }: { request: { agentKey: string } },
  ) => {
    reads.push(request.agentKey);
    const transcript = transcripts.get(request.agentKey);
    if (!transcript) throw { kind: "unavailable" };
    return {
      kind: transcript.kind,
      snapshot: {
        childThreadId: request.agentKey,
        parentThreadId: "parent",
        retentionLimited: false,
        turns: [{ id: "child-turn", status: transcript.status, items: [] }],
      },
    };
  },
}));

afterEach(() => {
  cleanup();
  transcripts.clear();
  reads.length = 0;
  resetSubagentLiveness();
});

/** Children spawned by a parent turn that ended while they still ran. */
function page(children: string[], runtimeEpoch: string | null = "2") {
  return {
    projectId: "project",
    chatId: "chat",
    currentSessionId: "session",
    sessions: [
      {
        id: "session",
        runtimeEpoch,
        capabilities: { childTranscriptRead: true },
      },
    ],
    turns: [
      {
        id: "turn",
        sessionId: "session",
        runtimeEpoch: "2",
        state: "completed",
        browserOperations: [],
        items: children.map((key) => ({
          id: `spawn-${key}`,
          kind: "collaboration",
          projectId: "project",
          chatId: "chat",
          sessionId: "session",
          payload: {
            operation: "spawnAgent",
            agentKeys: [key],
            agentStates: [[key, "running"]],
            receiverLabels: [key],
          },
        })),
      },
    ],
  } as unknown as AcpChatPage;
}

function renderStatuses(initial: AcpChatPage, epoch = 1) {
  const view = renderHook(
    ({ page, epoch }) =>
      useSettledSubagents(
        page,
        "3",
        projectRecentAgents(page, page.chatId),
        epoch,
      ).map((agent) => agent.status),
    { initialProps: { page: initial, epoch } },
  );
  return {
    statuses: () => view.result.current,
    rerender: (page: AcpChatPage, epoch: number) =>
      view.rerender({ page, epoch }),
  };
}

const keys = (count: number) =>
  Array.from({ length: count }, (_, index) => `child-${index}`);

it("settles every child once each read settles one, past the per-epoch read budget", async () => {
  const children = keys(10);
  for (const key of children)
    transcripts.set(key, { kind: "current", status: "completed" });
  const view = renderStatuses(page(children));
  await waitFor(() =>
    expect(view.statuses()).toEqual(children.map(() => "done")),
  );
  expect([...reads].sort()).toEqual([...children].sort());
});

it("shares reads among the unresolved children on later epochs, least recently read first", async () => {
  const children = keys(9);
  for (const key of children)
    transcripts.set(key, { kind: "current", status: "inProgress" });
  const view = renderStatuses(page(children));
  await waitFor(() => expect(reads).toHaveLength(8));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(reads).toHaveLength(8);
  const unread = children.find((key) => !reads.includes(key))!;
  transcripts.set(unread, { kind: "current", status: "failed" });
  expect(view.statuses()).toEqual(children.map(() => "working"));

  view.rerender(page(children), 2);
  await waitFor(() =>
    expect(view.statuses()[children.indexOf(unread)]).toBe("failed"),
  );
  expect(view.statuses().filter((status) => status === "working")).toHaveLength(
    8,
  );
  expect(reads[8]).toBe(unread);
});

it.each([
  ["completed", "restarted", "done", "9"],
  ["failed", "restarted", "failed", "9"],
  ["completed", "closed", "done", null],
  ["failed", "closed", "failed", null],
])(
  "keeps a %s child's outcome once its runtime has %s",
  async (status, _change, outcome, runtimeEpoch) => {
    transcripts.set("child-0", { kind: "current", status });
    const view = renderStatuses(page(["child-0"]));
    await waitFor(() => expect(view.statuses()).toEqual([outcome]));

    view.rerender(page(["child-0"], runtimeEpoch), 2);
    expect(view.statuses()).toEqual([outcome]);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(view.statuses()).toEqual([outcome]);
    expect(reads).toEqual(["child-0"]);
  },
);

it.each([
  ["completed", "done"],
  ["failed", "failed"],
  ["interrupted", "stopped"],
  ["inProgress", "stopped"],
])(
  "takes a %s child's outcome from its stored transcript once its runtime has closed",
  async (status, outcome) => {
    transcripts.set("child-0", { kind: "fromEarlierSession", status });
    const view = renderStatuses(page(["child-0"], null));
    await waitFor(() => expect(view.statuses()).toEqual([outcome]));
  },
);

it("stops a child whose runtime closed without a stored transcript", async () => {
  const view = renderStatuses(page(["child-0"], null));
  await waitFor(() => expect(view.statuses()).toEqual(["stopped"]));
  expect(reads).toEqual(["child-0"]);
});

it("keeps a live child working while its transcript is in progress", async () => {
  transcripts.set("child-0", { kind: "current", status: "inProgress" });
  const view = renderStatuses(page(["child-0"]));
  await waitFor(() => expect(reads).toEqual(["child-0"]));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(view.statuses()).toEqual(["working"]);
});
