import { afterEach, expect, it, vi } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.testFixture";
import { createAcpProjectionStore } from "../../../app/acpProjectionStore";
import type { AcpItem } from "../../../domain/acp";
import * as agentSummary from "./agentEditSummary";
import { projectChatEditSummary } from "./chatEditSummary";

afterEach(() => vi.restoreAllMocks());

it.each([
  [2, 3, 3, 4],
  [null, 3, null, 4],
  [2, null, 3, null],
])(
  "sums per-file totals across turns, preserving unknown sides (%s, %s)",
  (additions, deletions, totalAdditions, totalDeletions) => {
    const page = snapshotPage();
    const original = page.turns[0];
    const saved = original.items[0];
    if (saved.kind !== "fileChangeDiff") throw new Error("Expected saved edit");
    const latest = {
      ...original,
      id: "latest" as never,
      items: [
        {
          ...saved,
          turnId: "latest" as never,
          payload: {
            ...saved.payload,
            changes: [{ ...saved.payload.changes[0], additions, deletions }],
          },
        },
      ],
    };
    page.turns = [original, latest];
    const summary = projectChatEditSummary(page);
    expect(summary.changes).toHaveLength(1);
    expect(summary.changes[0]).toMatchObject({
      additions: totalAdditions,
      deletions: totalDeletions,
    });
    expect(summary.changes[0].turn).toBe(latest);
    expect(summary.changes[0].diffRef?.turnId).toBe("latest");
  },
);

it("does not re-project unchanged retained turns through accepted streaming pages", () => {
  const page = snapshotPage();
  const retained = page.turns[0];
  const saved = retained.items[0];
  if (saved.kind !== "fileChangeDiff") throw new Error("Expected saved edit");
  const active = {
    ...retained,
    id: "active" as never,
    ordinal: 2,
    state: "running" as const,
    items: [
      {
        ...saved,
        id: "stream" as never,
        turnId: "active" as never,
        kind: "text" as const,
        lifecycle: "streaming" as const,
        payload: { role: "assistant" as const, text: "first delta" },
      },
    ],
  };
  page.turns = [retained, active];
  const store = createAcpProjectionStore();
  const scope = {
    projectId: page.projectId,
    chatId: page.chatId,
    projectGeneration: "1" as never,
  };
  store.getState().activateScope(scope);
  expect(store.getState().applyAuthoritativePage(scope, page)).toBe("accepted");
  const initial = store.getState().page!;
  const project = vi.spyOn(agentSummary, "projectAgentEditSummary");
  expect(projectChatEditSummary(initial).changes).toHaveLength(1);
  for (const method of ["authoritative", "load"] as const) {
    project.mockClear();
    const delta = structuredClone(store.getState().page!);
    delta.turns = delta.turns.map(
      (turn) =>
        Object.fromEntries(Object.entries(turn).reverse()) as typeof turn,
    );
    delta.projectionRevision = String(
      Number(delta.projectionRevision) + 1,
    ) as never;
    const streaming = delta.turns[1];
    streaming.revision = String(Number(streaming.revision) + 1) as never;
    streaming.items = [
      {
        ...streaming.items[0],
        revision: streaming.revision,
        payload: {
          role: "assistant",
          text: "next delta " + streaming.revision,
        },
      } as AcpItem,
    ];
    const state = store.getState();
    expect(
      method === "load"
        ? state.completePageLoad(state.beginPageLoad(null), delta)
        : state.applyAuthoritativePage(scope, delta),
    ).toBe("accepted");
    const accepted = store.getState().page!;
    expect(projectChatEditSummary(accepted).changes).toHaveLength(1);
    expect(project).toHaveBeenCalledOnce();
    expect(project.mock.calls[0][1].id).toBe("active");
    expect(accepted.turns[0]).toBe(initial.turns[0]);
  }
  const delta = store.getState().page!;
  project.mockClear();
  projectChatEditSummary({ ...delta });
  expect(project).not.toHaveBeenCalled();
  expect(
    projectChatEditSummary({ ...delta, sessions: [] }).changes,
  ).toHaveLength(0);
  expect(
    projectChatEditSummary({ ...delta, chatId: "foreign" as never }).changes,
  ).toHaveLength(0);
  expect(
    projectChatEditSummary({ ...delta, projectId: "foreign" as never }).changes,
  ).toHaveLength(0);
});

it.each(["unsafe path", "nonfinite counts"])(
  "does not hide changed same-revision %s when accepting an authoritative page",
  (invalid) => {
    const page = snapshotPage();
    const original = page.turns[0].items[0];
    if (original.kind !== "fileChangeDiff") throw new Error("Expected edit");
    original.payload = {
      ...original.payload,
      changes: [{ ...original.payload.changes[0], additions: null }],
    };
    const store = createAcpProjectionStore();
    const scope = {
      projectId: page.projectId,
      chatId: page.chatId,
      projectGeneration: "1" as never,
    };
    store.getState().activateScope(scope);
    store.getState().applyAuthoritativePage(scope, page);
    expect(projectChatEditSummary(store.getState().page!).changes).toHaveLength(
      1,
    );
    const incoming = structuredClone(store.getState().page!);
    incoming.projectionRevision = "2" as never;
    const edit = incoming.turns[0].items[0];
    if (edit.kind !== "fileChangeDiff") throw new Error("Expected edit");
    edit.payload = {
      ...edit.payload,
      changes: [
        {
          ...edit.payload.changes[0],
          additions: invalid === "nonfinite counts" ? Number.NaN : null,
          resource:
            invalid === "unsafe path"
              ? {
                  kind: "resource",
                  resource: { rootId: "root-a", relativePath: "../unsafe.ts" },
                }
              : edit.payload.changes[0].resource,
        },
      ],
    };
    expect(store.getState().applyAuthoritativePage(scope, incoming)).toBe(
      "accepted",
    );
    const summary = projectChatEditSummary(store.getState().page!);
    expect(summary.changes).toHaveLength(0);
    expect(summary.rejectedUnsafeOrStale).toBe(true);
  },
);
