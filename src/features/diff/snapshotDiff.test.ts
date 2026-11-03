import { describe, expect, it } from "vitest";
import type { FileDiff } from "../../domain/workbench";
import { snapshotFileDiff } from "./snapshotDiff";

const resource = { rootId: "root", relativePath: "a.ts" };
const diff = (oldText: string, newText: string) =>
  snapshotFileDiff(resource, oldText, newText);

function expectHeadersMatchLines(result: FileDiff) {
  for (const hunk of result.hunks) {
    const match = /^@@ -(\d+),(\d+) \+(\d+),(\d+) @@$/.exec(hunk.header);
    expect(match, hunk.header).not.toBeNull();
    const [, , oldCount, , newCount] = match!.map(Number);
    expect({ header: hunk.header, oldCount, newCount }).toEqual({
      header: hunk.header,
      oldCount: hunk.lines.filter((line) => line.oldLine !== null).length,
      newCount: hunk.lines.filter((line) => line.newLine !== null).length,
    });
  }
}

const fixtures: [string, string][] = [
  ["start\nold\nend\n", "start\nnew\nend\n"],
  [`${"x\n".repeat(100_000)}old\n`, `${"x\n".repeat(100_000)}new\n`],
  [
    `old\n${"repeat\n".repeat(12)}old2\n`,
    `new\n${"repeat\n".repeat(12)}new2\n`,
  ],
  ["a", "a\n"],
  ["", "a\n"],
  ["a\n", ""],
  ["", "new\n".repeat(1200)],
  ["", "new\n".repeat(1201)],
  ["old\n".repeat(1000), "new\n".repeat(1000)],
];
describe("bounded immutable snapshot diff", () => {
  it("renders exact old/new lines with context and line numbers", () => {
    const result = diff("start\nold\nend\n", "start\nnew\nend\n");
    expect(result.truncated).toBe(false);
    expect(result.hunks).toEqual([
      {
        header: "@@ -1,3 +1,3 @@",
        lines: [
          { kind: "context", text: "start", oldLine: 1, newLine: 1 },
          { kind: "removed", text: "old", oldLine: 2, newLine: null },
          { kind: "added", text: "new", oldLine: null, newLine: 2 },
          { kind: "context", text: "end", oldLine: 3, newLine: 3 },
        ],
      },
    ]);
  });
  it("finds changes after a large equal prefix without rendering that prefix", () => {
    const prefix = "x\n".repeat(100_000);
    const result = diff(`${prefix}old\n`, `${prefix}new\n`);
    expect(result.hunks[0].header).toBe("@@ -99998,4 +99998,4 @@");
    expect(result.hunks[0].lines).toHaveLength(5);
    expect(result.truncated).toBe(false);
  });
  it("keeps repeated-line context and separates distant changes", () => {
    const unchanged = "repeat\n".repeat(12);
    const result = diff(`old\n${unchanged}old2\n`, `new\n${unchanged}new2\n`);
    expect(result.hunks).toHaveLength(2);
    expect(result.hunks[1].header).toBe("@@ -11,4 +11,4 @@");
  });
  it("distinguishes final newline edits and empty files", () => {
    expect(diff("", "").hunks).toEqual([]);
    expect(diff("a", "a\n").hunks[0].lines).toContainEqual({
      kind: "meta",
      text: " No newline at end of file",
      oldLine: null,
      newLine: null,
    });
    expect(diff("", "a\n").hunks[0].header).toBe("@@ -0,0 +1,1 @@");
    expect(diff("a\n", "").hunks[0].header).toBe("@@ -1,1 +0,0 @@");
  });
  it("enforces the exact display cap and truthfully marks plus one", () => {
    expect(diff("", "new\n".repeat(1200)).truncated).toBe(false);
    const result = diff("", "new\n".repeat(1201));
    expect(result.truncated).toBe(true);
    expect(result.hunks.flatMap((hunk) => hunk.lines)).toHaveLength(1200);
  });
  it("bounds quadratic work and rejects unadmitted raw sides", () => {
    const result = diff("old\n".repeat(1000), "new\n".repeat(1000));
    expect(result.hunks.flatMap((hunk) => hunk.lines)).toHaveLength(1200);
    expect(result.truncated).toBe(true);
    expect(() => diff("a".repeat(262145), "")).toThrow(/bounded/);
    expect(() => diff("é".repeat(131073), "")).toThrow(/bounded/);
  });
  it("re-heads a hunk clipped by the display cap to the lines shown", () => {
    const result = diff("", "new\n".repeat(1201));
    expect(result.hunks).toHaveLength(1);
    expect(result.hunks[0].header).toBe("@@ -0,0 +1,1200 @@");
    expect(result.hunks[0].lines).toHaveLength(1200);
    expect(result.truncated).toBe(true);
  });
  it("re-heads the clipped bounded-alignment hunk to its shown sides", () => {
    const result = diff("old\n".repeat(1000), "new\n".repeat(1000));
    expect(result.hunks.map((hunk) => hunk.header)).toEqual([
      "@@ -1,1000 +1,200 @@",
    ]);
    expectHeadersMatchLines(result);
  });
  it("keeps every hunk header's counts equal to its shown lines", () => {
    for (const [oldText, newText] of fixtures)
      expectHeadersMatchLines(diff(oldText, newText));
  });
});
