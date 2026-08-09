import { describe, expect, it } from "vitest";

import {
  MAX_FIND_MATCHES,
  MAX_FIND_QUERY_LENGTH,
  findLiteralMatches,
} from "./inFileFind";

describe("findLiteralMatches", () => {
  it("returns no matches for an empty query", () => {
    expect(findLiteralMatches("one two", "", false)).toEqual({
      offsets: [],
      truncated: false,
    });
  });

  it("finds non-overlapping literal matches with optional case folding", () => {
    expect(findLiteralMatches("Needle needle NEEDLE", "needle", true)).toEqual({
      offsets: [7],
      truncated: false,
    });
    expect(findLiteralMatches("Needle needle NEEDLE", "needle", false)).toEqual(
      { offsets: [0, 7, 14], truncated: false },
    );
  });

  it("caps the query and stored offsets", () => {
    expect(
      findLiteralMatches(
        "a".repeat(MAX_FIND_QUERY_LENGTH + 1),
        "a".repeat(MAX_FIND_QUERY_LENGTH + 1),
        true,
      ),
    ).toEqual({ offsets: [], truncated: false });

    const result = findLiteralMatches(
      "x ".repeat(MAX_FIND_MATCHES + 1),
      "x",
      true,
    );
    expect(result.offsets).toHaveLength(MAX_FIND_MATCHES);
    expect(result.truncated).toBe(true);
  });
});
