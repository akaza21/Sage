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

it("separates status scopes without mislabelling combined HEAD data", () => {
  const unstaged = projectChanges("unstaged", null, repositories).entries;
  expect(unstaged.map((entry) => entry.resource.relativePath)).toEqual([
    "unstaged.ts",
    "both.ts",
  ]);
  expect(unstaged[0].unavailable).toBeUndefined();
  expect(unstaged[1].unavailable).toMatch(/both staged and unstaged/);
  expect(unstaged[1].additions).toBeNull();
  expect(
    projectChanges("staged", null, repositories).entries.map(
      (entry) => entry.resource.relativePath,
    ),
  ).toEqual(["staged.ts", "both.ts"]);
});
