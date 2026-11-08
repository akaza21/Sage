import { describe, expect, it } from "vitest";
import { mentionQuery, mentionToken } from "./composerMention";

describe("composer @ mentions", () => {
  it("finds the @ token around the caret only at a word start", () => {
    expect(mentionQuery("Explain @src/ma", null)).toEqual({
      start: 8,
      end: 15,
      query: "src/ma",
    });
    // The caret inside the token still names the whole token for a pick.
    expect(mentionQuery("@src/main.rs please", 3)).toEqual({
      start: 0,
      end: 12,
      query: "src/main.rs",
    });
    expect(mentionQuery("line\n@", null)).toEqual({
      start: 5,
      end: 6,
      query: "",
    });
    expect(mentionQuery("mail bob@example.com", null)).toBeNull();
    expect(mentionQuery("@done next", null)).toBeNull();
    // A caret before the @ is not in the token.
    expect(mentionQuery("@src", 0)).toBeNull();
    // An opening quote typed by hand is not part of the query.
    expect(mentionQuery('@"docs', null)?.query).toBe("docs");
  });

  it("writes a token native reads back, quoting paths with spaces", () => {
    expect(mentionToken("src/main.rs")).toBe("@src/main.rs");
    expect(mentionToken("docs/my notes.md")).toBe('@"docs/my notes.md"');
  });
});
