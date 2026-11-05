import { expect, it } from "vitest";
import { snapshotPage } from "../../app/agentDiff.fixture";
import type { ProjectRepositoryStatus } from "../../domain/workbench";
import { projectChanges } from "./agentChanges";

const repositories: ProjectRepositoryStatus = {
  roots: [
    {
      rootId: "root-a",
      state: "repository",
      branch: "main",
      truncated: false,
      elapsedMs: 0,
      message: null,
      changes: [
        {
          resource: { rootId: "root-a", relativePath: "unstaged.ts" },
          indexStatus: "unchanged",
          worktreeStatus: "modified",
          additions: 2,
          deletions: 1,
        },
        {
          resource: { rootId: "root-a", relativePath: "staged.ts" },
          indexStatus: "added",
          worktreeStatus: "unchanged",
          additions: 3,
          deletions: 0,
        },
        {
          resource: { rootId: "root-a", relativePath: "both.ts" },
          indexStatus: "modified",
          worktreeStatus: "modified",
          additions: 5,
          deletions: 2,
        },
      ],
    },
  ],
  truncated: false,
  elapsedMs: 0,
};

it("uses only the latest current-session turn for Last turn", () => {
  const page = snapshotPage();
  expect(projectChanges("lastTurn", page, repositories).entries).toHaveLength(
    1,
  );
  const empty = {
    ...page.turns[0],
    id: "later" as never,
    ordinal: 2,
    items: [],
  };
  expect(
    projectChanges(
      "lastTurn",
      { ...page, turns: [...page.turns, empty] },
      repositories,
    ).entries,
  ).toEqual([]);
});

it("retains deleted files in delete-only and mixed Last turn summaries", () => {
  const page = snapshotPage();
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Missing edit fixture");
  const deleted = {
    ...item.payload.changes[0],
    pairState: undefined,
    snapshotRevision: undefined,
    contentDigest: undefined,
    oldBytes: undefined,
    newBytes: undefined,
    operation: "deleted" as const,
    additions: 0,
    deletions: 7,
    resource: {
      kind: "unavailable" as const,
      reason: "missing" as const,
      label: "src/deleted.ts",
      displayResource: { rootId: "root-a", relativePath: "src/deleted.ts" },
    },
  };
  const withChanges = (mixed: boolean) => ({
    ...page,
    turns: [
      {
        ...page.turns[0],
        items: [
          {
            ...item,
            payload: {
              ...item.payload,
              changes: mixed
                ? [
                    {
                      ...deleted,
                      operation: "updated" as const,
                      resource: {
                        kind: "resource" as const,
                        resource: {
                          rootId: "root-a",
                          relativePath: "src/kept.ts",
                        },
                      },
                    },
                    deleted,
                  ]
                : [deleted],
            },
          },
        ],
      },
    ],
  });
  expect(
    projectChanges("lastTurn", withChanges(false), repositories).entries,
  ).toEqual([
    expect.objectContaining({
      resource: deleted.resource.displayResource,
      additions: 0,
      deletions: 7,
      unavailable: expect.stringMatching(/deleted/i),
    }),
  ]);
  expect(
    projectChanges("lastTurn", withChanges(true), repositories).entries,
  ).toHaveLength(2);
});

it("separates status scopes without mislabelling combined HEAD data", () => {
  const unstaged = projectChanges("unstaged", null, repositories).entries;
  expect(unstaged.map((entry) => entry.resource.relativePath)).toEqual([
    "unstaged.ts",
    "both.ts",
  ]);
  expect(unstaged[0].unavailable).toBeUndefined();
  expect(unstaged[1].unavailable).toBeUndefined();
  expect(unstaged.map((entry) => entry.gitScope)).toEqual([
    "unstaged",
    "unstaged",
  ]);
  expect(unstaged[1].additions).toBeNull();
  expect(
    projectChanges("staged", null, repositories).entries.map(
      (entry) => entry.resource.relativePath,
    ),
  ).toEqual(["staged.ts", "both.ts"]);
});

it.each([false, true])(
  "keeps one canonical row for edit/delete transitions (recreated=%s)",
  (recreated) => {
    const page = snapshotPage();
    const item = page.turns[0].items[0];
    if (item.kind !== "fileChangeDiff") throw new Error("Missing edit fixture");
    const change = {
      ...item.payload.changes[0],
      pairState: undefined,
      snapshotRevision: undefined,
      contentDigest: undefined,
      oldBytes: undefined,
      newBytes: undefined,
    };
    const deleted = {
      ...change,
      operation: "deleted" as const,
      additions: 0,
      deletions: 7,
      resource: {
        kind: "unavailable" as const,
        reason: "missing" as const,
        label: "src/a.ts",
        displayResource: { rootId: "root-a", relativePath: "src/a.ts" },
      },
    };
    const first = recreated ? deleted : change;
    const latest = recreated
      ? { ...change, operation: "created" as const, additions: 3, deletions: 0 }
      : deleted;
    const turn = {
      ...page.turns[0],
      items: [
        { ...item, payload: { ...item.payload, changes: [first] } },
        {
          ...item,
          id: "later-item" as typeof item.id,
          ordinal: 2,
          payload: { ...item.payload, changes: [latest] },
        },
      ],
    };
    const result = projectChanges(
      "lastTurn",
      { ...page, turns: [turn] },
      repositories,
    );
    expect(result.entries).toHaveLength(1);
    expect(result.entries[0]).toMatchObject({
      resource: { rootId: "root-a", relativePath: "src/a.ts" },
      additions: recreated ? 3 : 0,
      deletions: recreated ? 0 : 7,
    });
  },
);
