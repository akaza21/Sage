import { describe, expect, it } from "vitest";
import MarkdownIt from "markdown-it";
import { boundedTable } from "./markdownTable";
import commonmark from "./fixtures/commonmark-0.31.2-blocks.json";
import gfm from "./fixtures/gfm-0.29-tables.json";
import { parseMarkdownTokens } from "./markdownTokens";
import { renderMarkdownPreview } from "./markdownPreview";

describe("bounded table admission", () => {
  const stock = new MarkdownIt();
  const bounded = new MarkdownIt();
  bounded.block.ruler.at("table", boundedTable, {
    alt: ["paragraph", "reference"],
  });

  function tableState(source: string) {
    return new bounded.block.State(source, bounded, {}, []);
  }

  it("admits only 12 of 50,000 columns before token allocation", () => {
    const source =
      "|h".repeat(50000) +
      "|\n" +
      "|-".repeat(50000) +
      "|\n" +
      "| x |\n\nAfter the table";
    const { tokens, truncated } = parseMarkdownTokens(source);
    expect(tokens).toHaveLength(85);
    expect(truncated).toBe(true);
    expect(tokens.at(-2)?.content).toBe("After the table");
    expect(renderMarkdownPreview(source).blocks).toEqual([
      {
        kind: "table",
        align: Array(12).fill(undefined),
        rows: [Array(12).fill("h"), ["x", ...Array(11).fill("")]],
      },
      { kind: "paragraph", text: "After the table" },
    ]);
  });

  it("bounds state.tokens for 50,000 columns in a single rule invocation", () => {
    const source =
      "|h".repeat(50000) + "|\n" + "|-".repeat(50000) + "|\n\nAfter";
    const state = tableState(source);
    expect(boundedTable(state, 0, state.lineMax, false)).toBe(true);
    expect(state.tokens).toHaveLength(42);
    expect(state.env.sageTableTruncated).toBe(true);
    expect(state.line).toBe(2);
    bounded.block.tokenize(state, state.line, state.lineMax);
    expect(state.tokens.at(-2)?.content).toBe("After");
  });

  it("bounds state.tokens for 40,000 rows in a single rule invocation", () => {
    const source = "|h|\n|-|\n" + "|x|\n".repeat(40000) + "\nAfter";
    const state = tableState(source);
    expect(boundedTable(state, 0, state.lineMax, false)).toBe(true);
    expect(state.tokens).toHaveLength(171);
    expect(state.env.sageTableTruncated).toBe(true);
    expect(state.line).toBe(40002);
    expect(state.tokens[0].map).toEqual([0, 40002]);
    bounded.block.tokenize(state, state.line, state.lineMax);
    expect(state.tokens.at(-2)?.content).toBe("After");
  });

  it("counts and skips 50,000 body cells beyond a narrow header", () => {
    const source = "|h|\n|-|\n" + "|x".repeat(50000) + "|\n\nAfter";
    const { tokens, truncated } = parseMarkdownTokens(source);
    expect(tokens).toHaveLength(19);
    expect(
      tokens
        .filter((token) => token.type === "inline")
        .map((token) => token.content),
    ).toEqual(["h", "x", "After"]);
    expect(truncated).toBe(true);
  });

  it("consumes the same source as stock at its ragged-row expansion limit", () => {
    const source =
      "|h".repeat(12) +
      "|\n" +
      "|-".repeat(12) +
      "|\n" +
      "x\n".repeat(6000) +
      "\nAfter";
    const expected = new stock.block.State(source, stock, {}, []);
    stock.block.ruler.getRules("")[0](expected, 0, expected.lineMax, false);
    const state = tableState(source);
    boundedTable(state, 0, state.lineMax, false);
    expect(state.tokens).toHaveLength(1260);
    expect(state.line).toBe(expected.line);
    expect(state.tokens[0].map).toEqual(expected.tokens[0].map);
    bounded.block.tokenize(state, state.line, state.lineMax);
    expect(state.tokens.at(-2)?.content).toBe("After");
  });

  it("validates omitted delimiter columns before admitting a table", () => {
    const source = "|h".repeat(50000) + "|\n" + "|-".repeat(49999) + "|:|\n";
    const state = tableState(source);
    expect(boundedTable(state, 0, state.lineMax, false)).toBe(false);
    expect(state.tokens).toEqual([]);
    expect(state.env.sageTableTruncated).toBeUndefined();
  });

  it("recognizes an overwide table silently without tokens or truncation", () => {
    const source = "|h".repeat(50000) + "|\n" + "|-".repeat(50000) + "|\n";
    const state = tableState(source);
    expect(boundedTable(state, 0, state.lineMax, true)).toBe(true);
    expect(state.tokens).toEqual([]);
    expect(state.line).toBe(0);
    expect(state.env.sageTableTruncated).toBeUndefined();
  });

  it.each(gfm.examples)(
    "matches stock tokens and spec HTML for GFM example $example",
    ({ markdown, html }) => {
      expect(bounded.parse(markdown, {})).toEqual(stock.parse(markdown, {}));
      expect(bounded.render(markdown)).toBe(stock.render(markdown));
      // The spec uses align attributes; markdown-it uses equivalent CSS styles.
      expect(bounded.render(markdown)).toBe(
        html.replace(/align="(left|center|right)"/gu, 'style="text-align:$1"'),
      );
    },
  );

  // CommonMark has no table extension. Its existing block examples protect
  // table rejection, paragraph interruption and container ownership as well.
  it.each(commonmark.examples)(
    "matches stock for CommonMark example $example",
    ({ markdown }) => {
      expect(bounded.parse(markdown, {})).toEqual(stock.parse(markdown, {}));
      expect(bounded.render(markdown)).toBe(stock.render(markdown));
    },
  );

  it.each([
    "| a\\|b | c |\n| :-- | --: |\n| a\\\\|b | c |\n\nAfter",
    "\u00a0| a | b |\u2003\n|-|-|\n\u3000| c | d |\ufeff\n\nAfter",
    "||\n|-|\n||\n\nAfter",
    "a\\|b\n:--\npipe-less\n\nAfter",
    "|a|b|\n|--||\nAfter",
    "|a|b|\n|-:|:-|\n|  | |\n\nAfter",
  ])("preserves escaped pipes, whitespace and edge cells: %j", (source) => {
    expect(bounded.parse(source, {})).toEqual(stock.parse(source, {}));
    expect(bounded.render(source)).toBe(stock.render(source));
  });
});
