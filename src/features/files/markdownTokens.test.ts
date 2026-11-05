import { describe, expect, it } from "vitest";
import { MAX_MARKDOWN_PREVIEW_BLOCKS } from "./markdownLimits";
import { renderMarkdownPreview } from "./markdownPreview";
import { parseMarkdownTokens } from "./markdownTokens";

describe("bounded markdown tokenization", () => {
  it.each(["- x\n", "> - x\n", "> > - x\n"])(
    "bounds a single huge container (%j) and closes it before skipping the tail",
    (item) => {
      const { tokens, truncated } = parseMarkdownTokens(
        item.repeat(65536) + "\n# skipped\n\n- skipped",
      );
      expect(tokens.length).toBeLessThanOrEqual(24600);
      const containers: string[] = [];
      for (const token of tokens) {
        if (token.nesting === 1) containers.push(token.tag);
        if (token.nesting === -1) expect(containers.pop()).toBe(token.tag);
      }
      expect(containers).toEqual([]);
      expect(tokens.some((token) => token.content.includes("skipped"))).toBe(
        false,
      );
      expect(truncated).toBe(true);
      expect(parseMarkdownTokens("# Next").truncated).toBe(false);
    },
  );

  it("keeps a huge plain quote to five balanced tokens and reports display truncation", () => {
    const source = "> x\n".repeat(65536);
    const { tokens } = parseMarkdownTokens(source);
    expect(tokens.map((token) => token.type)).toEqual([
      "blockquote_open",
      "paragraph_open",
      "inline",
      "paragraph_close",
      "blockquote_close",
    ]);
    expect(renderMarkdownPreview(source).truncated).toBe(true);
  });

  it("allocates tokens only for the admitted prefix of a 256 KiB answer", () => {
    const { tokens, truncated } = parseMarkdownTokens("# x\n".repeat(65536));
    expect(
      tokens.filter((token) => token.level === 0 && token.nesting >= 0),
    ).toHaveLength(MAX_MARKDOWN_PREVIEW_BLOCKS);
    expect(tokens).toHaveLength(192);
    expect(tokens.filter((token) => token.type === "inline")).toHaveLength(64);
    expect(truncated).toBe(true);
  });

  it.each([
    ["lists", "- a\n- b\n- c\n\n# split\n\n", 640],
    ["nested quotes", "> > # x\n\n# split\n\n", 320],
  ])(
    "stops before further %s without cutting admitted containers",
    (_, block, count) => {
      const { tokens, truncated } = parseMarkdownTokens(block.repeat(10000));
      expect(
        tokens.filter((token) => token.level === 0 && token.nesting >= 0),
      ).toHaveLength(MAX_MARKDOWN_PREVIEW_BLOCKS);
      expect(tokens).toHaveLength(count);
      expect(tokens.reduce((depth, token) => depth + token.nesting, 0)).toBe(0);
      expect(truncated).toBe(true);
    },
  );

  it("uses the remaining allowance when parsing a streaming suffix", () => {
    const { tokens, truncated } = parseMarkdownTokens("# x\n".repeat(65536), 2);
    expect(tokens).toHaveLength(6);
    expect(truncated).toBe(true);
    expect(parseMarkdownTokens("# skipped", 0)).toEqual({
      tokens: [],
      truncated: true,
    });
  });

  it("does not truncate at an exact cap or carry overflow into the next parse", () => {
    expect(parseMarkdownTokens("# x\n".repeat(65)).truncated).toBe(true);
    const exact = parseMarkdownTokens("# x\n".repeat(64) + "\n\t\n");
    expect(exact.tokens).toHaveLength(192);
    expect(exact.truncated).toBe(false);
    const next = parseMarkdownTokens("# Next");
    expect(next.tokens).toHaveLength(3);
    expect(next.truncated).toBe(false);
  });

  it("shows overflow without admitting formatting or links from skipped blocks", () => {
    const document = renderMarkdownPreview(
      "# x\n".repeat(64) + "# **skipped** [link](skipped.ts)",
    );
    expect(document.blocks).toHaveLength(64);
    expect(document.formats).toHaveLength(64);
    expect(document.formats.every(({ spans }) => spans.join("") === "x")).toBe(
      true,
    );
    expect(document.fileLinks).toEqual([]);
    expect(document.truncated).toBe(true);
  });
});
