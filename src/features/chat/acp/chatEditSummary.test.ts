import { afterEach, expect, it, vi } from "vitest";
import { snapshotPage } from "../../../app/agentDiff.fixture";
import { createAcpProjectionStore } from "../../../app/acpProjectionStore";
import type { AcpItem } from "../../../domain/acp";
import * as agentSummary from "./agentEditSummary";
import { projectChatEditSummary } from "./chatEditSummary";

afterEach(() => vi.restoreAllMocks());

it("deduplicates missing-resource harness deletions with ledger paths", () => {
  const page = snapshotPage();
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Expected edit");
  item.payload.changes = [
    {
      ...item.payload.changes[0],
      operation: "deleted",
      resource: {
        kind: "unavailable",
        reason: "missing",
        label: "src/a.ts",
        displayResource: { rootId: "root-a", relativePath: "src/a.ts" },
      },
    },
  ];
  const workspace = {
    turnId: page.turns[0].id,
    rootId: "root-a",
    relativePath: "src/a.ts",
    operation: "deleted" as const,
  };
  expect(projectChatEditSummary(page, [workspace]).changes).toHaveLength(1);
  const later = {
    ...page.turns[0],
    id: "later" as never,
    ordinal: 2,
    items: [],
  };
  page.turns = [...page.turns, later];
  const summary = projectChatEditSummary(page, [
    workspace,
    { ...workspace, turnId: later.id, operation: "created" },
  ]);
  expect(summary.changes).toHaveLength(1);
  expect(summary.changes[0]).toMatchObject({
    operation: "created",
    turn: { id: "later" },
  });
});

it("bounds shell-only rows and keeps the last record within a turn", () => {
  const page = snapshotPage();
  page.turns[0].items = [];
  const workspace = Array.from({ length: 300 }, (_, index) => ({
    turnId: page.turns[0].id,
    rootId: "root-a",
    relativePath: `shell-${index}.ts`,
    operation: "created" as const,
  }));
  const summary = projectChatEditSummary(page, [
    ...workspace,
    { ...workspace[299], operation: "deleted" },
  ]);
  expect(summary.changes).toHaveLength(32);
  expect(summary.omittedCount).toBe(268);
  expect(summary.changes.at(-1)).toMatchObject({
    operation: "deleted",
    resource: { resource: { relativePath: "shell-299.ts" } },
  });
});

it.each([
  [false, false],
  [false, true],
  [true, false],
  [true, true],
])(
  "uses the newest turn's shell record over older edits (harness: %s, reversed: %s)",
  (harness, reversed) => {
    const page = snapshotPage();
    const first = page.turns[0];
    const latest = { ...first, id: "latest" as never, ordinal: 2, items: [] };
    page.turns = [harness ? first : { ...first, items: [] }, latest];
    const workspace = [
      {
        turnId: first.id,
        rootId: "root-a",
        relativePath: "src/a.ts",
        operation: "created" as const,
      },
      {
        turnId: latest.id,
        rootId: "root-a",
        relativePath: "src/a.ts",
        operation: "deleted" as const,
      },
    ];
    const summary = projectChatEditSummary(
      page,
      reversed ? workspace.reverse() : workspace,
    );
    expect(summary.changes).toHaveLength(1);
    expect(summary.changes[0]).toMatchObject({
      operation: "deleted",
      turn: { id: "latest" },
    });
    expect(summary.changes[0].diffRef).toBeUndefined();
  },
);

it("caches at most 32 changes per turn while retaining keys for unique omissions", () => {
  const page = snapshotPage();
  const original = page.turns[0];
  const saved = original.items[0];
  if (saved.kind !== "fileChangeDiff") throw new Error("Expected saved edit");
  original.items = Array.from({ length: 40 }, (_, index) => ({
    ...saved,
    id: `edit-${index}` as never,
    ordinal: index,
    payload: {
      ...saved.payload,
      changes: [
        {
          ...saved.payload.changes[0],
          resource: {
            kind: "resource" as const,
            resource: {
              rootId: "root-a",
              relativePath: `src/file-${index}.ts`,
            },
          },
        },
      ],
    },
  }));
  const project = vi.spyOn(agentSummary, "projectAgentEditSummary");
  expect(projectChatEditSummary(page).omittedCount).toBe(8);
  const cached = project.mock.results[0].value;
  expect(cached.changes).toHaveLength(32);
  expect(cached.changeKeys.size).toBe(40);
  project.mockClear();
  expect(projectChatEditSummary({ ...page }).omittedCount).toBe(8);
  expect(project).not.toHaveBeenCalled();
  page.turns = [
    original,
    {
      ...original,
      id: "latest" as never,
      ordinal: 2,
      items: [{ ...original.items[0], turnId: "latest" as never }],
    },
  ];
  const summary = projectChatEditSummary(page);
  expect(summary.changes).toHaveLength(32);
  expect(summary.omittedCount).toBe(8);
  expect(summary.changes.at(-1)?.turn.id).toBe("latest");
});

it.each([
  [2, 3, 2, 3],
  [null, 3, null, 3],
  [2, null, 2, null],
])(
  "uses the latest diff's per-file totals, preserving unknown sides (%s, %s)",
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

it("adds shell-made workspace changes beside harness edits, harness records winning", () => {
  const page = snapshotPage();
  const turn = page.turns[0];
  const saved = turn.items[0];
  if (saved.kind !== "fileChangeDiff") throw new Error("Expected saved edit");
  const harnessPath = (
    saved.payload.changes[0].resource as {
      resource: { relativePath: string; rootId: string };
    }
  ).resource;
  const summary = projectChatEditSummary(page, [
    {
      turnId: turn.id,
      rootId: harnessPath.rootId,
      relativePath: harnessPath.relativePath,
      operation: "updated",
    },
    {
      turnId: turn.id,
      rootId: harnessPath.rootId,
      relativePath: "README.md",
      operation: "updated",
    },
    {
      turnId: "unknown-turn",
      rootId: harnessPath.rootId,
      relativePath: "ignored.md",
      operation: "created",
    },
  ]);
  expect(summary.changes).toHaveLength(2);
  const shell = summary.changes.find(
    (change) =>
      change.resource.kind === "resource" &&
      change.resource.resource.relativePath === "README.md",
  );
  expect(shell?.operation).toBe("updated");
  expect(shell?.diffRef).toBeUndefined();
  expect(shell?.turn.id).toBe(turn.id);
  const harness = summary.changes.find(
    (change) =>
      change.resource.kind === "resource" &&
      change.resource.resource.relativePath === harnessPath.relativePath,
  );
  expect(harness?.diffRef).toBeDefined();
});
