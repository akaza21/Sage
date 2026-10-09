import { describe, expect, it } from "vitest";
import {
  highlightCodeSource,
  MAX_CODE_HIGHLIGHT_TOKENS,
} from "../../files/CodeMirrorEditorRuntime";

describe("static code highlighting", () => {
  it("discards obsolete work before parsing a loaded language", async () => {
    let calls = 0;
    const spans = await highlightCodeSource(
      "return 1",
      "python",
      () => ++calls === 1,
    );
    expect(calls).toBe(2);
    expect(spans).toBeNull();
  });
  it.each([
    "python",
    "typescript",
    "javascript",
    "javascript-jsx",
    "typescript-jsx",
    "json",
    "css",
    "html",
    "markdown",
    "rust",
  ] as const)(
    "uses the existing %s parser without changing source",
    async (language) => {
      const source =
        language === "python"
          ? 'def valid(value):\n  # comment\n  return "literal"'
          : 'const value = "literal";';
      const spans = await highlightCodeSource(source, language);
      expect(spans).not.toBeNull();
      expect(spans!.length).toBeLessThanOrEqual(MAX_CODE_HIGHLIGHT_TOKENS);
      expect(
        spans!.every((span) => span.from >= 0 && span.to <= source.length),
      ).toBe(true);
      if (language === "python") expect(spans!.length).toBeGreaterThan(3);
    },
  );

  it("admits the exact token bound and fails closed at plus one", async () => {
    expect(
      await highlightCodeSource(
        "1\n".repeat(MAX_CODE_HIGHLIGHT_TOKENS),
        "python",
      ),
    ).toHaveLength(MAX_CODE_HIGHLIGHT_TOKENS);
    expect(
      await highlightCodeSource(
        "1\n".repeat(MAX_CODE_HIGHLIGHT_TOKENS + 1),
        "python",
      ),
    ).toBeNull();
    expect(await highlightCodeSource("x".repeat(4097), "python")).toBeNull();
    expect(await highlightCodeSource("plain", "plain")).toBeNull();
  });
});
