import { describe, expect, it } from "vitest";

import {
  boundedExpandedUpdate,
  boundedRecordUpdate,
  boundedResourceStateUpdate,
} from "./resourceStateHelpers";

describe("bounded record updates", () => {
  it("moves rewritten keys newest and drops the oldest beyond the limit", () => {
    const next = boundedRecordUpdate({ a: 1, b: 2, c: 3 }, ["a", "d"], 9, 3);
    expect(Object.entries(next)).toEqual([
      ["c", 3],
      ["a", 9],
      ["d", 9],
    ]);
  });

  it("never mutates the current record", () => {
    const current = { a: true };
    expect(boundedExpandedUpdate(current, ["b"], 1)).toEqual({ b: true });
    expect(current).toEqual({ a: true });
  });

  it("keeps a single resource state newest", () => {
    const loading = { phase: "loading" } as const;
    const next = boundedResourceStateUpdate(
      { x: loading, y: loading },
      "x",
      loading,
      2,
    );
    expect(Object.keys(next)).toEqual(["y", "x"]);
  });
});
