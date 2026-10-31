import { ChangeSet, Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import {
  HUNK_ADDED,
  HUNK_DELETED,
  HUNK_MODIFIED,
  applyChanges,
  createGutterWorkerCore,
} from "./gutterWorkerCore";

describe("gutter worker core", () => {
  it("diffs the opened document, then follows edits and base changes", () => {
    const core = createGutterWorkerCore();
    const base = "one\r\ntwo\r\nthree\r\n";
    const opened = core.handle({
      type: "open",
      id: 1,
      version: 1,
      base,
      doc: "one\nTWO\nthree\n",
    });
    // Offsets point into the staged text as stored, CRLF included.
    expect(opened).toEqual({
      id: 1,
      version: 1,
      hunks: [1, 2, 5, 8, HUNK_MODIFIED],
      truncated: false,
    });
    expect(base.slice(5, 8)).toBe("two");

    const doc = Text.of(["one", "TWO", "three", ""]);
    const insert = ChangeSet.of(
      { from: doc.length, insert: "four\n" },
      doc.length,
    );
    expect(
      core.handle({ type: "edit", id: 1, version: 2, changes: insert.toJSON() })
        ?.hunks,
    ).toEqual([1, 2, 5, 8, HUNK_MODIFIED, 3, 4, 17, 17, HUNK_ADDED]);

    expect(
      core.handle({
        type: "base",
        id: 1,
        version: 3,
        base: "zero\none\nTWO\nthree\nfour\n",
      })?.hunks,
    ).toEqual([0, 0, 0, 4, HUNK_DELETED]);

    expect(core.handle({ type: "close", id: 1 })).toBeNull();
    expect(core.size()).toBe(0);
    expect(
      core.handle({ type: "edit", id: 1, version: 4, changes: [] }),
    ).toBeNull();
  });
});

describe("applyChanges", () => {
  it("matches CodeMirror on random edits", () => {
    let seed = 11;
    const random = (limit: number) => {
      seed = (seed * 1_103_515_245 + 12_345) & 0x7fffffff;
      return seed % limit;
    };
    let doc = Text.of(["alpha", "beta", "", "gamma"]);
    let text = doc.toString();
    for (let round = 0; round < 300; round += 1) {
      const from = random(doc.length + 1);
      const to = Math.min(doc.length, from + random(4));
      const insert = ["", "x", "\n", "y\nz", "é\n\n"][random(5)]!;
      const changes = ChangeSet.of({ from, to, insert }, doc.length);
      const composed = changes.compose(
        ChangeSet.of({ from: 0, insert: "q" }, changes.newLength),
      );
      text = applyChanges(text, composed.toJSON());
      doc = composed.apply(doc);
      expect(text).toBe(doc.toString());
    }
  });

  it("refuses changes that do not fit the text", () => {
    expect(() => applyChanges("abc", [2])).toThrow();
    expect(() => applyChanges("abc", ["x"])).toThrow();
  });
});
