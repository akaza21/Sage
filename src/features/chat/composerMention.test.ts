import { describe, expect, it } from "vitest";
import {
  mentionQuery,
  mentionRootQualifier,
  mentionToken,
} from "./composerMention";

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
    // An opening parenthesis may lead the token and keeps its closer.
    expect(mentionQuery("see (@src/a.rs)", 10)).toEqual({
      start: 5,
      end: 14,
      query: "src/a.rs",
    });
    expect(mentionQuery("f(@src", null)).toBeNull();
  });

  it("keeps a quoted token open across its spaces", () => {
    expect(mentionQuery('Read @"my no', null)).toEqual({
      start: 5,
      end: 12,
      query: "my no",
    });
    // The caret inside a closed quote names the whole token.
    expect(mentionQuery('@"my notes.md" next', 5)).toEqual({
      start: 0,
      end: 14,
      query: "my notes.md",
    });
    expect(mentionQuery('@"my notes.md" next', 14)).toBeNull();
    expect(mentionQuery('@"my notes.md" next', null)).toBeNull();
    expect(mentionQuery('@"one\ntwo', null)).toBeNull();
  });

  it("writes a token native reads back, quoting paths with spaces", () => {
    expect(mentionToken("src/main.rs")).toBe("@src/main.rs");
    expect(mentionToken("docs/my notes.md")).toBe('@"docs/my notes.md"');
    expect(mentionToken("src/a.rs", "lib")).toBe("@lib:src/a.rs");
    expect(mentionToken("a b.rs", "my lib")).toBe('@"my lib:a b.rs"');
    // A quote inside a quoted token, or a colon anywhere, cannot be read back.
    expect(mentionToken('say "hi".md')).toBeNull();
    expect(mentionToken('"lead.md')).toBeNull();
    expect(mentionToken("a:b.md")).toBeNull();
    expect(mentionToken('odd"name.md')).toBe('@odd"name.md');
  });

  it("names a root other than the first by a name only it has, else by id", () => {
    const roots = [
      { id: "a", name: "app" },
      { id: "b", name: "lib" },
      { id: "c", name: "app" },
      { id: "d", name: "x:y" },
    ];
    expect(mentionRootQualifier(roots, "a")).toBeNull();
    expect(mentionRootQualifier(roots, "b")).toBe("lib");
    expect(mentionRootQualifier(roots, "c")).toBe("c");
    expect(mentionRootQualifier(roots, "gone")).toBe("gone");
    expect(mentionRootQualifier(roots, "d")).toBe("d");
    expect(mentionRootQualifier([], "a")).toBeNull();
  });
});
