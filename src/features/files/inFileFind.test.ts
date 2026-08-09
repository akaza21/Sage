import { describe, expect, it } from "vitest";

import {
  MAX_EDITABLE_CONTENT_BYTES,
  MAX_FIND_MATCHES,
  MAX_FIND_QUERY_LENGTH,
  findLiteralMatches,
} from "./inFileFind";

describe("findLiteralMatches", () => {
  it("returns no matches for an empty query", () => {
    expect(findLiteralMatches("one two", "", false)).toEqual({
      offsets: [],
      truncated: false,
      contentLimitExceeded: false,
    });
  });

  it("finds non-overlapping literal matches with optional case folding", () => {
    expect(findLiteralMatches("Needle needle NEEDLE", "needle", true)).toEqual({
      offsets: [7],
      truncated: false,
      contentLimitExceeded: false,
    });
    expect(findLiteralMatches("Needle needle NEEDLE", "needle", false)).toEqual(
      {
        offsets: [0, 7, 14],
        truncated: false,
        contentLimitExceeded: false,
      },
    );
  });

  it("caps the query and stored offsets", () => {
    expect(
      findLiteralMatches(
        "a".repeat(MAX_FIND_QUERY_LENGTH + 1),
        "a".repeat(MAX_FIND_QUERY_LENGTH + 1),
        true,
      ),
    ).toEqual({
      offsets: [],
      truncated: false,
      contentLimitExceeded: false,
    });

    const result = findLiteralMatches(
      "x ".repeat(MAX_FIND_MATCHES + 1),
      "x",
      true,
    );
    expect(result.offsets).toHaveLength(MAX_FIND_MATCHES);
    expect(result.truncated).toBe(true);
  });

  it("accepts the 1 MiB editor ceiling and rejects plus one before matching", () => {
    const exactAscii = `${"a".repeat(MAX_EDITABLE_CONTENT_BYTES - 1)}z`;
    expect(findLiteralMatches(exactAscii, "z", true)).toEqual({
      offsets: [MAX_EDITABLE_CONTENT_BYTES - 1],
      truncated: false,
      contentLimitExceeded: false,
    });
    expect(findLiteralMatches(`${exactAscii}z`, "z", true)).toEqual({
      offsets: [],
      truncated: false,
      contentLimitExceeded: true,
    });

    const exactUtf8 = "é".repeat(MAX_EDITABLE_CONTENT_BYTES / 2);
    expect(findLiteralMatches(exactUtf8, "z", true)).toMatchObject({
      contentLimitExceeded: false,
    });
    expect(findLiteralMatches(`${exactUtf8}é`, "z", true)).toEqual({
      offsets: [],
      truncated: false,
      contentLimitExceeded: true,
    });
  });
});
