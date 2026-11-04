import { expect, it } from "vitest";
import type { FileChangeProjection } from "../../domain/acp";
import { snapshotPage } from "../../app/agentDiff.fixture";
import { changesSnapshot, snapshotChanges } from "./changesSnapshot";

it("keeps capped payload edits omitted when the native ledger also lists them", () => {
  const page = snapshotPage();
  const item = page.turns[0].items[0];
  if (item.kind !== "fileChangeDiff") throw new Error("Missing edit fixture");
  const original = item.payload.changes[0];
  const items = Array.from({ length: 33 }, (_, index) => {
    const change: FileChangeProjection =
      index === 32
        ? {
            ...original,
            pairState: "unavailable",
            snapshotRevision: "0",
            oldBytes: null,
            newBytes: null,
            contentDigest: null,
            resource: {
              kind: "unavailable",
              reason: "foreign",
              label: "Outside project",
            },
          }
        : {
            ...original,
            resource: {
              kind: "resource",
              resource: {
                rootId: "root-a",
                relativePath: `src/edit-${index}.ts`,
              },
            },
          };
    return {
      ...item,
      id: `item-${index}` as typeof item.id,
      ordinal: index + 1,
      payload: { ...item.payload, changes: [change] },
    };
  });
  const snapshot = changesSnapshot({
    ...page,
    turns: [{ ...page.turns[0], items }],
  });
  const result = snapshotChanges(snapshot, {
    changes: [
      {
        turnId: page.turns[0].id,
        rootId: "root-a",
        relativePath: "src/edit-0.ts",
        operation: "updated",
      },
    ],
    incompleteTurnIds: new Set(),
  });
  expect(result.entries).toHaveLength(31);
  expect(
    result.entries.some(
      (entry) => entry.resource.relativePath === "src/edit-0.ts",
    ),
  ).toBe(false);
  expect(result.entries[0].resource.relativePath).toBe("src/edit-1.ts");
  expect(result.truncated).toBe(true);
});

it("combines shell-only edits with retained diff descriptors, preferring the retained edit", () => {
  const page = snapshotPage();
  const result = snapshotChanges(changesSnapshot(page), {
    changes: ["src/a.ts", "src/shell.ts"].map((relativePath) => ({
      turnId: page.turns[0].id,
      rootId: "root-a",
      relativePath,
      operation: "updated",
    })),
    incompleteTurnIds: new Set(),
  });
  expect(result.entries.map((entry) => entry.resource.relativePath)).toEqual([
    "src/shell.ts",
    "src/a.ts",
  ]);
  expect(result.entries[1].diffRef?.itemId).toBe("item-a");
  expect(result.entries[1].additions).toBe(1);
  expect(result.truncated).toBe(false);
});
