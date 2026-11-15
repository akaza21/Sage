import { describe, expect, it } from "vitest";
import MarkdownIt from "markdown-it";
import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_TABLE_ROWS,
} from "./markdownLimits";
import {
  renderMarkdownPreview,
  type MarkdownPreviewBlock,
} from "./markdownPreview";
import { markdownTextLocations } from "./markdownTextLocations";
import { parseMarkdownTokens } from "./markdownTokens";

describe("nested markdown content", () => {
  it("retains a quoted warning inside a list item", () => {
    expect(renderMarkdownPreview("- > quoted warning").blocks).toEqual([
      {
        kind: "list",
        items: [
          {
            children: [
              {
                kind: "quote",
                children: [{ kind: "paragraph", text: "quoted warning" }],
              },
            ],
          },
        ],
      },
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
    expect(document.blocks).toEqual([
      {
        kind: "list",
        items: [{ children: [{ kind: "paragraph", text: "warning" }] }],
      },
    ]);
    expect(document.truncated).toBe(false);
  });

  it("retains task states in a quote's list children", () => {
    expect(
      renderMarkdownPreview("> - [x] done\n> - [ ] pending").blocks,
    ).toEqual([
      {
        kind: "quote",
        children: [
          {
            kind: "list",
            items: [
              {
                checked: true,
                children: [{ kind: "paragraph", text: "done" }],
              },
              {
                checked: false,
                children: [{ kind: "paragraph", text: "pending" }],
              },
            ],
          },
        ],
      },
    ]);
  });

  it("retains ordered starts and nested numbering inside a quote", () => {
    expect(
      renderMarkdownPreview(
        "> 42. release\n> 43. next\n>\n>     7. nested\n>     8. nested-next\n> 44. last",
      ).blocks,
    ).toEqual([
      {
        kind: "quote",
        children: [
          {
            kind: "list",
            ordered: true,
            start: 42,
            loose: true,
            items: [
              { children: [{ kind: "paragraph", text: "release" }] },
              {
                children: [
                  { kind: "paragraph", text: "next" },
                  {
                    kind: "list",
                    ordered: true,
                    start: 7,
                    items: [
                      { children: [{ kind: "paragraph", text: "nested" }] },
                      {
                        children: [{ kind: "paragraph", text: "nested-next" }],
                      },
                    ],
                  },
                ],
              },
              { children: [{ kind: "paragraph", text: "last" }] },
            ],
          },
        ],
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
    if (block.kind === "list")
      return block.items.flatMap((item) => item.children.map(output)).join(" ");
    if (block.kind === "quote") return block.children.map(output).join(" ");
    if (block.kind === "table") return block.rows.flat().join(" ");
    return "text" in block ? block.text : "";
  }
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
          ...chain.map((_, index) => `before-${index}`),
          ...leaf.text,
          ...chain.map((_, index) => `after-${index}`).reverse(),
        ],
      })),
    ),
  )("retains all text through $name", ({ source, expected }) => {
    const document = renderMarkdownPreview(source);
    const text = document.blocks.map(output).join(" ");
    let previous = -1;
    for (const value of expected) {
      expect(text.indexOf(value)).toBeGreaterThan(previous);
      previous = text.indexOf(value);
      expect(text).toContain(value);
      expect(text.split(value)).toHaveLength(2);
    }
    expect(document.fileLinks).toEqual([]);
    expect(document.truncated).toBe(false);
  });
});

describe("bounded markdown tokenization", () => {
  it("bounds empty list items whose fast path does not recurse into block rules", () => {
    const { tokens, truncated } = parseMarkdownTokens("-\n\n".repeat(40000));
    expect(tokens.length).toBeLessThanOrEqual(24600);
    expect(tokens.reduce((depth, token) => depth + token.nesting, 0)).toBe(0);
    expect(truncated).toBe(true);
  });

  it.each([33, 40000])(
    "allocates only the display prefix of a table with %i body rows",
    (rows) => {
      const source = "| H |\n| - |\n" + "| x |\n".repeat(rows);
      const { tokens, truncated } = parseMarkdownTokens(source);
      // Header plus 32 body rows, including their balanced table sections.
      expect(tokens).toHaveLength(171);
      expect(truncated).toBe(true);
      expect(renderMarkdownPreview(source).truncated).toBe(true);
    },
  );

  it("skips omitted table rows without swallowing the following paragraph", () => {
    const document = renderMarkdownPreview(
      "| H |\n| - |\n" + "| x |\n".repeat(40000) + "\nAfter the table",
    );
    expect(document.blocks).toEqual([
      {
        kind: "table",
        align: [undefined],
        rows: [["H"], ...Array.from({ length: 32 }, () => ["x"])],
      },
      { kind: "paragraph", text: "After the table" },
    ]);
    expect(document.truncated).toBe(true);
  });

  it.each([
    ["# After", "heading"],
    ["- After", "list"],
    ["> After", "quote"],
    ["```\nAfter\n```", "code"],
    ["    After", "code"],
    ["\nAfter", "paragraph"],
  ])("preserves the table terminator %j as a %s", (tail, kind) => {
    const document = renderMarkdownPreview(
      "| H |\n| - |\n" + "| x |\n".repeat(100) + tail,
    );
    expect(document.blocks.map((block) => block.kind)).toEqual(["table", kind]);
    expect(document.truncated).toBe(true);
  });

  it.each(["quote", "list"])(
    "bounds a table within a %s and retains prose on both sides of its boundary",
    (kind) => {
      const body = "| H |\n| - |\n" + "| x |\n".repeat(100) + "\nInside";
      const nested = body
        .split("\n")
        .map((line) => (kind === "quote" ? `> ${line}` : `  ${line}`))
        .join("\n");
      const source =
        (kind === "list" ? "- Before\n\n" : "") + nested + "\n\nOutside";
      const { tokens, truncated } = parseMarkdownTokens(source);
      expect(tokens.length).toBeLessThan(200);
      expect(
        tokens
          .filter((token) => token.type === "inline")
          .slice(-2)
          .map((token) => token.content),
      ).toEqual(["Inside", "Outside"]);
      expect(truncated).toBe(true);
      const document = renderMarkdownPreview(source);
      expect(document.blocks.map((block) => block.kind)).toEqual([
        kind,
        "paragraph",
      ]);
      expect(document.blocks.at(-1)).toEqual({
        kind: "paragraph",
        text: "Outside",
      });
    },
  );

  it.each([0, 1, MAX_MARKDOWN_TABLE_ROWS])(
    "leaves normal table tokens unchanged with %i body rows",
    (rows) => {
      const source =
        "| Left | Right |\n| :-- | --: |\n" + "| a\\|b | c |\n".repeat(rows);
      const stock = new MarkdownIt();
      stock.core.ruler.disable([
        "inline",
        "linkify",
        "replacements",
        "smartquotes",
        "text_join",
      ]);
      expect(parseMarkdownTokens(source)).toEqual({
        tokens: stock.parse(source, {}),
        truncated: false,
      });
      expect(renderMarkdownPreview(source).truncated).toBe(false);
    },
  );

  it.each(["- before\n\n  ```text\n  ", "> before\n>\n> ```text\n> "])(
    "shares the character allowance with nested literal code: %j",
    (prefix) => {
      const document = renderMarkdownPreview(prefix + "x".repeat(5000));
      expect(
        document.blocks
          .flatMap((block) =>
            [...markdownTextLocations(block)].map((location) => location.text),
          )
          .join(""),
      ).toBe("before" + "x".repeat(4090));
      expect(document.truncated).toBe(true);
      expect(document.fileLinks).toEqual([]);
    },
  );
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
