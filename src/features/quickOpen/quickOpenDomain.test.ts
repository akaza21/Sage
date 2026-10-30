import { describe, expect, it } from "vitest";

import {
  highlightRuns,
  nextQuickOpenStamp,
  parseQuickOpenInput,
  parseQuickOpenResponse,
} from "./quickOpenDomain";

describe("Quick Open input", () => {
  it("splits a trailing :line or :line:column from the query", () => {
    expect(parseQuickOpenInput("src/app.ts:12")).toEqual({
      query: "src/app.ts",
      line: 12,
      column: null,
    });
    expect(parseQuickOpenInput(" app :7:3 ")).toEqual({
      query: "app",
      line: 7,
      column: 3,
    });
    expect(parseQuickOpenInput(":40")).toEqual({
      query: "",
      line: 40,
      column: null,
    });
    // Not a position: a colon inside the name, or line 0.
    expect(parseQuickOpenInput("a:b")).toMatchObject({
      query: "a:b",
      line: null,
    });
    expect(parseQuickOpenInput("app:0")).toMatchObject({
      query: "app",
      line: null,
    });
  });

  it("caps the query at the native 1,024 bytes", () => {
    const { query } = parseQuickOpenInput("é".repeat(600));
    expect(new TextEncoder().encode(query).length).toBeLessThanOrEqual(1024);
    expect(query.length).toBe(512);
  });
});

describe("Quick Open stamps", () => {
  it("start from the clock and always increase", () => {
    const first = nextQuickOpenStamp(1_000);
    expect(first).toBeGreaterThanOrEqual(1_000_000);
    expect(nextQuickOpenStamp(1_000)).toBe(first + 1);
    expect(nextQuickOpenStamp(500)).toBe(first + 2);
  });
});

describe("Quick Open responses", () => {
  const ready = {
    state: "ready",
    results: [{ rootId: "r", relativePath: "src/app.ts", positions: [4, 5] }],
    indexedFiles: 10,
    truncated: false,
    built: true,
    refreshing: false,
    indexBytes: 100,
    readyMs: 3,
  };

  it("keeps the fields the UI uses", () => {
    expect(parseQuickOpenResponse(ready)).toEqual({
      state: "ready",
      results: ready.results,
      indexedFiles: 10,
      truncated: false,
      refreshing: false,
    });
  });

  it("rejects malformed responses", () => {
    for (const bad of [
      null,
      { ...ready, state: "done" },
      {
        ...ready,
        results: [{ rootId: "r", relativePath: "a", positions: [1] }],
      },
      { ...ready, results: Array(51).fill(ready.results[0]) },
      { ...ready, refreshing: "no" },
    ]) {
      expect(() => parseQuickOpenResponse(bad)).toThrow(
        "invalid Quick Open response",
      );
    }
  });
});

describe("highlight runs", () => {
  it("marks matched characters by their offset in the whole path", () => {
    expect(highlightRuns("app.ts", 4, [4, 5, 7])).toEqual([
      { text: "ap", matched: true },
      { text: "p", matched: false },
      { text: ".", matched: true },
      { text: "ts", matched: false },
    ]);
  });
});
