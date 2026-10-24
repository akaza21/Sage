import { describe, expect, it } from "vitest";
import {
  attachedContextKey,
  attachContextToDraft,
  parseAttachedContext,
  validateAttachedContexts,
} from "./attachedContext";
import { isTranscriptQuote } from "./transcriptQuote";

const quote = {
  kind: "transcriptQuote" as const,
  ordinal: 1,
  sourceTurnId: "turn-1",
  sourceItemId: "item-1",
  text: "Selected text",
  startOffset: 2,
  endOffset: 15,
};

describe("transcript attached context", () => {
  it("admits bounded quotes and stable source keys", () => {
    expect(parseAttachedContext(quote)).toEqual(quote);
    expect(attachedContextKey(quote)).toBe("quote:item-1:2-15");
    expect(
      parseAttachedContext({ ...quote, text: "x".repeat(4097) }),
    ).toBeNull();
    expect(
      parseAttachedContext({ ...quote, comment: "x".repeat(4097) }),
    ).toBeNull();
    expect(parseAttachedContext({ ...quote, endOffset: 1 })).toBeNull();
    expect(parseAttachedContext({ ...quote, ordinal: 0 })).toBeNull();
    expect(parseAttachedContext({ ...quote, text: " " })).toBeNull();
    expect(
      parseAttachedContext({ ...quote, sourceItemId: "x".repeat(513) }),
    ).toBeNull();
    expect(
      parseAttachedContext({ ...quote, sourceTurnId: "turn\u0085" }),
    ).toBeNull();
    expect(parseAttachedContext({ ...quote, extra: true })).toBeNull();
    expect(parseAttachedContext({ ...quote, ordinal: 9 })).toBeNull();
    expect(
      parseAttachedContext({ ...quote, text: "水".repeat(1366) }),
    ).toBeNull();
    expect(parseAttachedContext({ ...quote, comment: "" })).toEqual({
      ...quote,
      comment: "",
    });
    expect(isTranscriptQuote(quote)).toBe(true);
  });
  it("caps a draft at eight", () => {
    expect(() =>
      validateAttachedContexts(Array.from({ length: 9 }, () => quote)),
    ).toThrow();
  });
  it("edits a duplicate in place at the cap and keeps the draft order", () => {
    const contexts = validateAttachedContexts(
      Array.from({ length: 8 }, (_, index) => ({
        ...quote,
        sourceItemId: `item-${index}`,
        ordinal: index + 1,
      })),
    );
    const edited = attachContextToDraft(contexts, {
      ...quote,
      sourceItemId: "item-3",
      comment: "Updated",
    });
    expect(edited).toHaveLength(8);
    expect(edited[3]).toMatchObject({
      sourceItemId: "item-3",
      comment: "Updated",
    });
    expect(() =>
      attachContextToDraft(edited, { ...quote, sourceItemId: "new-item" }),
    ).toThrow();
    // Display numbers follow position, so deletion needs no renumbering pass.
    const deleted = edited.filter((_, index) => index !== 2);
    expect(deleted[2]).toMatchObject({
      sourceItemId: "item-3",
      comment: "Updated",
    });
  });
});
