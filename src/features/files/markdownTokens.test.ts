import { describe, expect, it } from "vitest";
import { MAX_MARKDOWN_PREVIEW_BLOCKS } from "./markdownLimits";
import {
  renderMarkdownPreview,
  type MarkdownPreviewBlock,
} from "./markdownPreview";
import { parseMarkdownTokens } from "./markdownTokens";

describe("nested markdown content", () => {
  it("retains a quoted warning inside a list item", () => {
    expect(renderMarkdownPreview("- > quoted warning").blocks).toEqual([
      { kind: "list", items: ["quoted warning"] },
    ]);
  });

  it.each([8, 16, 17, 18, 32, 64])(
    "retains text when %i quote levels reach the parser nesting limit",
    (depth) => {
      const document = renderMarkdownPreview(
        "> ".repeat(depth) + "deep warning",
      );
      expect(document.blocks.map(output).join(" ")).toContain("deep warning");
      expect(document.truncated).toBe(false);
    },
  );

  it("does not let markup-only items consume the visible list allowance", () => {
    const document = renderMarkdownPreview(
      "- <br>\n".repeat(4096) + "- warning",
    );
    expect(document.blocks).toEqual([{ kind: "list", items: ["warning"] }]);
    expect(document.truncated).toBe(false);
  });

  it("retains task states when a quote cannot present list structure", () => {
    expect(
      renderMarkdownPreview("> - [x] done\n> - [ ] pending").blocks,
    ).toEqual([{ kind: "quote", text: "[x] done [ ] pending" }]);
  });

  it("retains ordered starts and numbering when a quote flattens nested lists", () => {
    expect(
      renderMarkdownPreview(
        "> 42. release\n> 43. next\n>\n>     7. nested\n>     8. nested-next\n> 44. last",
      ).blocks,
    ).toEqual([
      {
        kind: "quote",
        text: "42. release 43. next 7. nested 8. nested-next 44. last",
      },
    ]);
  });

  const wrappers = [
    {
      name: "list",
      wrap: (text: string, id: string) =>
        `- before-${id}\n\n${text
          .split("\n")
          .map((line) => `  ${line}`)
          .join("\n")}\n\n  after-${id}`,
    },
    {
      name: "ordered list",
      wrap: (text: string, id: string) =>
        `3. before-${id}\n\n${text
          .split("\n")
          .map((line) => `   ${line}`)
          .join("\n")}\n\n   after-${id}`,
    },
    {
      name: "quote",
      wrap: (text: string, id: string) =>
        `before-${id}\n\n${text}\n\nafter-${id}`
          .split("\n")
          .map((line) => `> ${line}`)
          .join("\n"),
    },
  ];
  const leaves = [
    { name: "paragraph", markdown: "leaf-text", text: ["leaf-text"] },
    {
      name: "code",
      markdown: "```text\nliteral **stars** [code](hidden.ts) <tag>\n```",
      text: ["literal **stars** [code](hidden.ts) <tag>"],
    },
    {
      name: "indented code",
      markdown: "    literal **stars** <tag>",
      text: ["literal **stars** <tag>"],
    },
    {
      name: "table",
      markdown:
        "| header-one | header-two |\n| - | - |\n| cell-one | cell-two |",
      text: ["header-one", "header-two", "cell-one", "cell-two"],
    },
    { name: "heading", markdown: "## heading-text", text: ["heading-text"] },
    { name: "rule", markdown: "---", text: ["---"] },
  ];
  function output(block: MarkdownPreviewBlock): string {
    if (block.kind === "rule") return "---";
    if (block.kind === "list") return block.items.join(" ");
    if (block.kind === "table") return block.rows.flat().join(" ");
    return "text" in block ? block.text : "";
  }
  // Every container pair, including mixed list kinds, is exercised in both
  // directions and with a third enclosing container. Code and tables are leaves.
  const chains = wrappers.flatMap((outer) => [
    [outer],
    ...wrappers.flatMap((inner) => [
      [outer, inner],
      ...wrappers.map((third) => [outer, inner, third]),
    ]),
  ]);
  it.each(
    chains.flatMap((chain) =>
      leaves.map((leaf) => ({
        name: [...chain.map((wrapper) => wrapper.name), leaf.name].join(" / "),
        source: chain.reduceRight(
          (text, wrapper, index) => wrapper.wrap(text, String(index)),
          leaf.markdown,
        ),
        expected: [
          ...leaf.text,
          ...chain.flatMap((_, index) => [`before-${index}`, `after-${index}`]),
        ],
      })),
    ),
  )("retains all text through $name", ({ source, expected }) => {
    const document = renderMarkdownPreview(source);
    const text = document.blocks.map(output).join(" ");
    for (const value of expected) {
      expect(text).toContain(value);
      expect(text.split(value)).toHaveLength(2);
    }
    expect(document.fileLinks).toEqual([]);
    expect(document.truncated).toBe(false);
  });
});

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
