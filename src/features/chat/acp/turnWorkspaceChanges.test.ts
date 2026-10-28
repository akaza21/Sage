import { expect, it } from "vitest";

import { decodeTurnWorkspaceChanges } from "./turnWorkspaceChanges";

it("decodes bounded native workspace changes for one turn", () => {
  expect(
    decodeTurnWorkspaceChanges(
      {
        rootId: "root-a",
        tracked: true,
        truncated: false,
        changes: [
          { relativePath: "README.md", operation: "updated" },
          { relativePath: "src/new.ts", operation: "created" },
        ],
      },
      "turn-1",
    ),
  ).toEqual([
    {
      turnId: "turn-1",
      rootId: "root-a",
      relativePath: "README.md",
      operation: "updated",
    },
    {
      turnId: "turn-1",
      rootId: "root-a",
      relativePath: "src/new.ts",
      operation: "created",
    },
  ]);
});

it.each([
  ["../escape.txt"],
  ["C:/abs.txt"],
  ["/abs.txt"],
  ["a\\b.txt"],
  ["a//b.txt"],
])("rejects an unsafe path %s", (relativePath) => {
  expect(
    decodeTurnWorkspaceChanges(
      {
        rootId: "root-a",
        tracked: true,
        truncated: false,
        changes: [{ relativePath, operation: "updated" }],
      },
      "turn-1",
    ),
  ).toBeNull();
});

it("rejects unknown operations and oversized replies", () => {
  const reply = (changes: unknown[]) => ({
    rootId: "root-a",
    tracked: true,
    truncated: false,
    changes,
  });
  expect(
    decodeTurnWorkspaceChanges(
      reply([{ relativePath: "a.txt", operation: "renamed" }]),
      "t",
    ),
  ).toBeNull();
  expect(
    decodeTurnWorkspaceChanges(
      reply(
        Array.from({ length: 301 }, (_, index) => ({
          relativePath: `f${index}`,
          operation: "updated",
        })),
      ),
      "t",
    ),
  ).toBeNull();
  expect(decodeTurnWorkspaceChanges(null, "t")).toBeNull();
});
