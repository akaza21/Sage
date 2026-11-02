import { describe, expect, it } from "vitest";

import { diffLines, type LineChange } from "./lineDiff";

const lines = (text: string) => text.split("\n");

/** Rebuilds `current` from `base` and the changes, proving the script valid. */
function apply(
  base: readonly string[],
  current: readonly string[],
  changes: readonly LineChange[],
) {
  const out: string[] = [];
  let cursor = 0;
  for (const change of changes) {
    // The same number of unchanged lines precedes the change on both sides.
    expect(change.baseStart - cursor).toBe(change.start - out.length);
    out.push(...base.slice(cursor, change.baseStart));
    out.push(...current.slice(change.start, change.end));
    cursor = change.baseEnd;
  }
  out.push(...base.slice(cursor));
  return out;
}

function lcs(a: readonly string[], b: readonly string[]) {
  const row = new Array<number>(b.length + 1).fill(0);
  for (let i = 1; i <= a.length; i += 1) {
    let diagonal = 0;
    for (let j = 1; j <= b.length; j += 1) {
      const above = row[j]!;
      row[j] =
        a[i - 1] === b[j - 1] ? diagonal + 1 : Math.max(row[j]!, row[j - 1]!);
      diagonal = above;
    }
  }
  return row[b.length]!;
}

const changed = (changes: readonly LineChange[]) =>
  changes.reduce(
    (sum, change) =>
      sum + change.end - change.start + change.baseEnd - change.baseStart,
    0,
  );

describe("diffLines", () => {
  it.each([
    ["no change", "a\nb\nc", "a\nb\nc", []],
    [
      "added at the start",
      "b\nc",
      "a\nb\nc",
      [{ start: 0, end: 1, baseStart: 0, baseEnd: 0 }],
    ],
    [
      "added at the end",
      "a\nb",
      "a\nb\nc",
      [{ start: 2, end: 3, baseStart: 2, baseEnd: 2 }],
    ],
    [
      "modified in the middle",
      "a\nb\nc",
      "a\nB\nc",
      [{ start: 1, end: 2, baseStart: 1, baseEnd: 2 }],
    ],
    [
      "deleted at the start",
      "a\nb\nc",
      "b\nc",
      [{ start: 0, end: 0, baseStart: 0, baseEnd: 1 }],
    ],
    [
      "deleted at the end",
      "a\nb\nc",
      "a\nb",
      [{ start: 2, end: 2, baseStart: 2, baseEnd: 3 }],
    ],
    [
      "a final line break removed",
      "a\n",
      "a",
      [{ start: 1, end: 1, baseStart: 1, baseEnd: 2 }],
    ],
    [
      "everything replaced",
      "a\nb",
      "x\ny\nz",
      [{ start: 0, end: 3, baseStart: 0, baseEnd: 2 }],
    ],
    [
      "two separate changes",
      "a\nb\nc\nd\ne",
      "a\nB\nc\nd\nE\nf",
      [
        { start: 1, end: 2, baseStart: 1, baseEnd: 2 },
        { start: 4, end: 6, baseStart: 4, baseEnd: 5 },
      ],
    ],
  ])("%s", (_name, base, current, expected) => {
    expect(diffLines(lines(base), lines(current))).toEqual(expected);
  });

  it("finds a shortest edit script on random texts", () => {
    let seed = 7;
    const random = () => {
      seed = (seed * 1_103_515_245 + 12_345) & 0x7fffffff;
      return seed / 0x7fffffff;
    };
    const text = () =>
      Array.from({ length: Math.floor(random() * 30) }, () =>
        "abcd".charAt(Math.floor(random() * 4)),
      );
    for (let round = 0; round < 500; round += 1) {
      const base = text();
      const current = text();
      const changes = diffLines(base, current);
      expect(apply(base, current, changes)).toEqual(current);
      expect(changed(changes)).toBe(
        base.length + current.length - 2 * lcs(base, current),
      );
      for (const change of changes) {
        expect(
          change.end - change.start + change.baseEnd - change.baseStart,
        ).toBeGreaterThan(0);
      }
    }
  });

  it("stays valid and bounded on large unrelated texts", () => {
    const base = Array.from({ length: 20_000 }, (_, index) => `base ${index}`);
    const current = Array.from(
      { length: 20_000 },
      (_, index) => `${index % 3 === 0 ? "base" : "now"} ${index}`,
    );
    const started = performance.now();
    const changes = diffLines(base, current);
    expect(performance.now() - started).toBeLessThan(2_000);
    expect(apply(base, current, changes)).toEqual(current);
  });
});
