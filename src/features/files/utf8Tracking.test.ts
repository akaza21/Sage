import { EditorState, Text } from "@codemirror/state";
import { describe, expect, it, vi } from "vitest";

import {
  documentUtf8Length,
  utf8Length,
  utf8LengthField,
  withinUtf8Limit,
} from "./utf8Tracking";

const encoder = new TextEncoder();
const bytesOf = (value: string) => encoder.encode(value).byteLength;

const PIECES = ["a", "\n", "é", "€", "😀", "\ud83d", "\ude00", "中", " "];

function random(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

describe("UTF-8 tracking", () => {
  it("counts exactly as TextEncoder, lone surrogates included", () => {
    const next = random(7);
    for (let round = 0; round < 500; round += 1) {
      let value = "";
      const length = Math.floor(next() * 40);
      for (let index = 0; index < length; index += 1) {
        value += PIECES[Math.floor(next() * PIECES.length)];
      }
      expect(utf8Length(value)).toBe(bytesOf(value));
      expect(documentUtf8Length(Text.of(value.split("\n")))).toBe(
        bytesOf(value),
      );
    }
  });

  it("keeps the tracked length equal to the real one through random edits, even ones that split surrogate pairs", () => {
    const next = random(11);
    let state = EditorState.create({
      doc: "start 😀 € é\nline two 中文",
      extensions: utf8LengthField,
    });
    for (let step = 0; step < 2_000; step += 1) {
      const length = state.doc.length;
      const from = Math.floor(next() * (length + 1));
      const to = Math.min(length, from + Math.floor(next() * 4));
      let insert = "";
      const pieces = Math.floor(next() * 3);
      for (let index = 0; index < pieces; index += 1) {
        insert += PIECES[Math.floor(next() * PIECES.length)];
      }
      const second = Math.floor(next() * (length + 1));
      const changes =
        next() < 0.2 && (second < from || second > to)
          ? [
              { from, to, insert },
              { from: second, to: second, insert: "😀" },
            ]
          : [{ from, to, insert }];
      state = state.update({ changes }).state;
      expect(state.field(utf8LengthField)).toBe(bytesOf(state.doc.toString()));
    }
  });

  it("rejects an edit that would cross the byte limit without reading the document", () => {
    const limit = 30;
    const state = EditorState.create({
      doc: "€".repeat(9),
      extensions: utf8LengthField,
    });
    const fits = state.update({ changes: { from: 0, insert: "abc" } });
    const crosses = state.update({ changes: { from: 0, insert: "€€" } });
    const toString = vi.spyOn(Text.prototype, "toString");
    expect(withinUtf8Limit(fits, limit)).toBe(true);
    expect(withinUtf8Limit(crosses, limit)).toBe(false);
    expect(toString).not.toHaveBeenCalled();
    toString.mockRestore();
  });

  it("falls back to measuring when the length field is not installed", () => {
    const state = EditorState.create({ doc: "€".repeat(9) });
    const crosses = state.update({ changes: { from: 0, insert: "€€" } });
    expect(withinUtf8Limit(crosses, 30)).toBe(false);
    expect(withinUtf8Limit(crosses, 33)).toBe(true);
  });
});
