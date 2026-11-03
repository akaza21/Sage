import { describe, expect, it, vi } from "vitest";

import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  MAX_CHAT_FILE_LINKS,
  MAX_CHAT_FILE_LINK_PATH_BYTES,
  MAX_MARKDOWN_LIST_DEPTH,
  canPreviewDiagram,
  canPreviewMarkdown,
  renderMarkdownPreview,
} from "./markdownPreview";

describe("canPreviewMarkdown", () => {
  it("accepts only markdown path languages", () => {
    expect(canPreviewMarkdown("docs/readme.md")).toBe(true);
    expect(canPreviewMarkdown("NOTES.markdown")).toBe(true);
    expect(canPreviewMarkdown("src/app.ts")).toBe(false);
    expect(canPreviewMarkdown("")).toBe(false);
  });
});

describe("canPreviewDiagram", () => {
  it("accepts standalone mermaid paths only", () => {
    expect(canPreviewDiagram("docs/flow.mmd")).toBe(true);
    expect(canPreviewDiagram("docs/flow.mermaid")).toBe(true);
    expect(canPreviewDiagram("docs/readme.md")).toBe(false);
    expect(canPreviewDiagram("src/app.ts")).toBe(false);
  });
});

describe("renderMarkdownPreview", () => {
  it("bounds delimiter candidate checks with repeated opening brackets", () => {
    const started = performance.now();
    const document = renderMarkdownPreview("**" + "[".repeat(100000) + "]**");
    expect(performance.now() - started).toBeLessThan(1000);
    expect(document.truncated).toBe(true);
    expect(document.blocks[0]).toMatchObject({ kind: "paragraph" });
  });
  it("fails closed before rejected delimiters can repeatedly scan the source suffix", () => {
    const original = String.prototype.indexOf;
    let searches = 0;
    const search = vi
      .spyOn(String.prototype, "indexOf")
      .mockImplementation(function (
        this: string,
        marker: string,
        from?: number,
      ) {
        if (marker === "**" && ++searches > 1000)
          throw new Error("Unbounded delimiter search");
        return original.call(this, marker, from);
      });
    let result: ReturnType<typeof renderMarkdownPreview> | undefined;
    let failure: unknown;
    try {
      result = renderMarkdownPreview("**a ".repeat(50000));
    } catch (error) {
      failure = error;
    } finally {
      search.mockRestore();
    }
    expect(failure).toBeUndefined();
    expect(searches).toBeLessThanOrEqual(1000);
    expect(result?.truncated).toBe(true);
  });
  it.each(["my_var_name", "2 * 3 * 4", "see my_file and [x](src/a_b.ts)"])(
    "respects delimiter flanking in %s without consuming links",
    (source) => {
      const document = renderMarkdownPreview(source);
      expect(
        document.formats[0].spans.some(
          (span) => typeof span !== "string" && "tag" in span,
        ),
      ).toBe(false);
      expect(document.fileLinks.map((link) => link.destination)).toEqual(
        source.includes("[x]") ? ["src/a_b.ts"] : [],
      );
    },
  );
  it("supports short table delimiters and indented list fences", () => {
    const document = renderMarkdownPreview(
      "1. example\n   ```python\n   print(1)\n   ```\n\n| Name | Value |\n| - | :-: |\n| x | y |",
    );
    expect(document.blocks.map((block) => block.kind)).toEqual([
      "list",
      "code",
      "table",
    ]);
    expect(document.blocks[1]).toMatchObject({
      text: "print(1)",
      language: "python",
      fenceClosed: true,
    });
    expect(document.blocks[2]).toMatchObject({ align: [undefined, "center"] });
  });
  it("bounds literal markup list items and their formatting records", () => {
    const document = renderMarkdownPreview(
      "- `<div>`\n".repeat(10000) + "- **Retained**",
    );
    expect(document.truncated).toBe(true);
    const block = document.blocks[0];
    expect(block.kind).toBe("list");
    if (block.kind === "list")
      expect(block.items.join("")).toHaveLength(
        MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
      );
    expect(document.formats.length).toBeLessThanOrEqual(
      MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
    );
  });
  it("keeps root-relative paths eligible for native admission but rejects UNC links", () => {
    const document = renderMarkdownPreview(
      String.raw`[local](\README.md) [unc](\\server\README.md)`,
    );
    expect(document.fileLinks.map((link) => link.destination)).toEqual([
      String.raw`\README.md`,
    ]);
  });
  it("retains inline code markup and whitespace before sanitizing prose", () => {
    const document = renderMarkdownPreview(
      "Use `<div>` and `a  b` and `<script>run()</script>`.",
    );
    expect(document.blocks[0]).toMatchObject({
      text: "Use <div> and a  b and <script>run()</script>.",
    });
    expect(
      document.formats[0].spans.filter(
        (span) => typeof span !== "string" && "tag" in span,
      ),
    ).toEqual([
      { tag: "code", children: ["<div>"] },
      { tag: "code", children: ["a  b"] },
      { tag: "code", children: ["<script>run()</script>"] },
    ]);
  });

  it("retains shorter backtick lines inside a longer literal fence", () => {
    const text = "```\n<script>run()</script>\n<style>body {}</style>";
    expect(
      renderMarkdownPreview(`\`\`\`\`html\n${text}\n\`\`\`\``).blocks,
    ).toEqual([{ kind: "code", language: "html", text, fenceClosed: true }]);
  });
  it("normalizes CRLF while preserving exact block/source limits", () => {
    expect(
      renderMarkdownPreview(
        "```html\r\n<script>run()</script>\r\nline  \r\n```\r\n",
      ).blocks[0],
    ).toMatchObject({ text: "<script>run()</script>\nline  " });
    expect(
      renderMarkdownPreview("x".repeat(MAX_MARKDOWN_PREVIEW_SOURCE_BYTES))
        .sourceLimitExceeded,
    ).toBe(false);
    expect(
      renderMarkdownPreview("x".repeat(MAX_MARKDOWN_PREVIEW_SOURCE_BYTES + 1))
        .sourceLimitExceeded,
    ).toBe(true);
    expect(
      renderMarkdownPreview(Array(64).fill("# H").join("\n")).truncated,
    ).toBe(false);
    expect(
      renderMarkdownPreview(Array(65).fill("# H").join("\n")).truncated,
    ).toBe(true);
  });
  it("retains literal fenced scripts and whitespace without admitting code links", () => {
    const source =
      "<script>run()</script>\n  <style>body {}</style>  \n[readme](README.md)";
    const document = renderMarkdownPreview(`\`\`\`html\n${source}\n\`\`\``);
    expect(document.blocks[0]).toMatchObject({ kind: "code", text: source });
    expect(document.fileLinks).toEqual([]);
  });

  it("parses ordered lists and aligned tables with escaped pipes and file links", () => {
    const document = renderMarkdownPreview(
      "3. **Database**\n4. `validator`\n\n| Name | Detail |\n| :--- | ---: |\n| **SQLite** | a\\|b [readme](README.md:7) |",
    );
    expect(document.blocks[0]).toMatchObject({
      kind: "list",
      ordered: true,
      start: 3,
      items: ["Database", "validator"],
    });
    expect(document.blocks[1]).toEqual({
      kind: "table",
      align: ["left", "right"],
      rows: [
        ["Name", "Detail"],
        ["SQLite", "a|b readme"],
      ],
    });
    expect(document.fileLinks[0]).toMatchObject({
      blockIndex: 1,
      itemIndex: 3,
      label: "readme",
      destination: "README.md:7",
      offset: 4,
    });
  });

  it("bounds tables at 32 body rows, 12 columns and 4096 retained characters", () => {
    const table = (rows: number, columns = 1) =>
      [
        Array(columns).fill("H").join(" | "),
        Array(columns).fill("---").join(" | "),
        ...Array.from({ length: rows }, () =>
          Array(columns).fill("cell").join(" | "),
        ),
      ]
        .map((row) => `| ${row} |`)
        .join("\n");
    expect(renderMarkdownPreview(table(32)).truncated).toBe(false);
    expect(renderMarkdownPreview(table(33)).truncated).toBe(true);
    expect(renderMarkdownPreview(table(1, 12)).truncated).toBe(false);
    expect(renderMarkdownPreview(table(1, 13)).truncated).toBe(true);
    expect(
      renderMarkdownPreview(`| H |\n| --- |\n| ${"x".repeat(4095)} |`)
        .truncated,
    ).toBe(false);
    expect(
      renderMarkdownPreview(`| H |\n| --- |\n| ${"x".repeat(4096)} |`)
        .truncated,
    ).toBe(true);
  });
  it("retains only bounded file-link candidates outside code fences", () => {
    const document = renderMarkdownPreview(
      [
        "See [readme](README.md), [line](src/main.ts:7), and [absolute](C:\\project\\src\\file.ts).",
        "[web](https://example.com) [foreign](file:///C:/secret) [traverse](../secret) [unc](\\\\server\\share)",
        "```md",
        "[code](README.md)",
        "```",
        `[huge](${"x".repeat(MAX_CHAT_FILE_LINK_PATH_BYTES + 1)})`,
        ...Array.from(
          { length: MAX_CHAT_FILE_LINKS + 2 },
          (_, index) => `[file${index}](f${index}.ts)`,
        ),
      ].join("\n"),
    );
    expect(
      document.fileLinks.map(({ label, destination }) => ({
        label,
        destination,
      })),
    ).toEqual([
      { label: "readme", destination: "README.md" },
      { label: "line", destination: "src/main.ts:7" },
      { label: "absolute", destination: "C:\\project\\src\\file.ts" },
      ...Array.from({ length: MAX_CHAT_FILE_LINKS - 3 }, (_, index) => ({
        label: `file${index}`,
        destination: `f${index}.ts`,
      })),
    ]);
    expect(document.fileLinks[0]).toMatchObject({ blockIndex: 0, offset: 4 });
  });
  it("projects a bounded CommonMark subset without HTML authority", () => {
    const document = renderMarkdownPreview(
      [
        "# Title",
        "",
        "Hello **world** and [docs](https://example.test).",
        "",
        "- one",
        "- two",
        "",
        "> quoted",
        "",
        "```",
        "code <em>line</em>",
        "```",
        "",
        "<script>alert(1)</script>",
        "![diagram](https://example.test/a.png)",
      ].join("\n"),
    );

    expect(document.sourceLimitExceeded).toBe(false);
    expect(document.truncated).toBe(false);
    expect(document.blocks).toEqual([
      { kind: "heading", level: 1, text: "Title" },
      {
        kind: "paragraph",
        text: "Hello world and docs.",
      },
      { kind: "list", items: ["one", "two"] },
      { kind: "quote", text: "quoted" },
      {
        kind: "code",
        text: "code <em>line</em>",
        language: "",
        fenceClosed: true,
      },
      { kind: "paragraph", text: "alert(1) diagram" },
    ]);
    // An image is never fetched, and its address is not shown.
    expect(JSON.stringify(document)).not.toContain("a.png");
    expect(JSON.stringify(document)).toContain("alert(1)");
  });

  it("nests indented list items under the item before them", () => {
    const document = renderMarkdownPreview(
      [
        "- one",
        "  - one.a",
        "    1. deep",
        "  - one.b",
        "- two",
        " - sibling, indented one space",
        "1. other list",
      ].join("\n"),
    );
    expect(document.blocks).toEqual([
      {
        kind: "list",
        items: [
          "one",
          "one.a",
          "deep",
          "one.b",
          "two",
          "sibling, indented one space",
        ],
        shape: [
          { depth: 0, ordered: false },
          { depth: 1, ordered: false },
          { depth: 2, ordered: true },
          { depth: 1, ordered: false },
          { depth: 0, ordered: false },
          { depth: 0, ordered: false },
        ],
      },
      { kind: "list", items: ["other list"], ordered: true, start: 1 },
    ]);
  });

  it("keeps the list a tree when an item is dropped or indented unevenly", () => {
    const [dropped] = renderMarkdownPreview(
      ["- <br>", "  - child", "    - grandchild", "- sibling"].join("\n"),
    ).blocks;
    expect(dropped).toMatchObject({
      items: ["child", "grandchild", "sibling"],
      shape: [{ depth: 0 }, { depth: 1 }, { depth: 0 }],
    });
    const [uneven] = renderMarkdownPreview(
      ["- a", "    - b", "  1. c"].join("\n"),
    ).blocks;
    expect(uneven).toMatchObject({
      items: ["a", "b", "c"],
      shape: [
        { depth: 0, ordered: false },
        { depth: 1, ordered: false },
        { depth: 1, ordered: true },
      ],
    });
  });

  it("refuses web link addresses with hidden characters", () => {
    const document = renderMarkdownPreview(
      "[a](https://example.test/\u202Egnp.exe) [b](https://exa\u200Bmple.test)",
    );
    expect(JSON.stringify(document.formats)).not.toContain('"tag":"a"');
  });

  it("bounds list nesting", () => {
    const lines = Array.from(
      { length: MAX_MARKDOWN_LIST_DEPTH + 4 },
      (_, depth) => `${"  ".repeat(depth)}- level ${depth}`,
    );
    const block = renderMarkdownPreview(lines.join("\n")).blocks[0];
    expect(block.kind === "list" && block.shape?.at(-1)?.depth).toBe(
      MAX_MARKDOWN_LIST_DEPTH - 1,
    );
  });

  it("reads task list items as checked or unchecked", () => {
    const document = renderMarkdownPreview(
      ["- [ ] todo", "- [x] done", "- [X] also done", "- [] not a task"].join(
        "\n",
      ),
    );
    expect(document.blocks).toEqual([
      {
        kind: "list",
        items: ["todo", "done", "also done", "[] not a task"],
        shape: [
          { depth: 0, ordered: false, checked: false },
          { depth: 0, ordered: false, checked: true },
          { depth: 0, ordered: false, checked: true },
          { depth: 0, ordered: false },
        ],
      },
    ]);
  });

  it("keeps a list item that interrupts a paragraph", () => {
    expect(
      renderMarkdownPreview("Steps:\n  + first\n  + second").blocks,
    ).toEqual([
      { kind: "paragraph", text: "Steps:" },
      { kind: "list", items: ["first", "second"] },
    ]);
  });

  it("keeps a web link's address without making it a File link", () => {
    const document = renderMarkdownPreview(
      '[docs](https://example.test/guide "Guide") [mail](<mailto:a@example.test>) [js](javascript:void)',
    );
    expect(document.blocks).toEqual([
      { kind: "paragraph", text: "docs mail js" },
    ]);
    expect(document.formats[0].spans).toEqual([
      "",
      { tag: "a", href: "https://example.test/guide", children: ["docs"] },
      " ",
      { tag: "a", href: "mailto:a@example.test", children: ["mail"] },
      " ",
      "js",
      "",
    ]);
    expect(document.fileLinks).toEqual([]);
  });

  it("retains fenced code languages for mermaid projection", () => {
    const document = renderMarkdownPreview(
      ["```mermaid", "graph TD", "A --> B", "```"].join("\n"),
    );
    expect(document.blocks).toEqual([
      {
        kind: "code",
        text: "graph TD\nA --> B",
        language: "mermaid",
        fenceClosed: true,
      },
    ]);
  });

  it("marks an unfinished fence on the exact retained code block", () => {
    const document = renderMarkdownPreview("```mermaid\ngraph TD\nA -->");
    expect(document.blocks).toEqual([
      {
        kind: "code",
        text: "graph TD\nA -->",
        language: "mermaid",
        fenceClosed: false,
      },
    ]);
  });

  it("treats CRLF the same as LF", () => {
    expect(renderMarkdownPreview("# Title\r\n\r\nHello")).toEqual(
      renderMarkdownPreview("# Title\n\nHello"),
    );
  });

  it("caps blocks and source bytes fail-closed", () => {
    const overflow = Array.from(
      { length: MAX_MARKDOWN_PREVIEW_BLOCKS + 1 },
      (_, index) => `# H${index + 1}`,
    ).join("\n\n");
    const truncated = renderMarkdownPreview(overflow);
    expect(truncated.blocks).toHaveLength(MAX_MARKDOWN_PREVIEW_BLOCKS);
    expect(truncated.truncated).toBe(true);

    const oversized = renderMarkdownPreview(
      `${"a".repeat(MAX_MARKDOWN_PREVIEW_SOURCE_BYTES + 8)}\n\n# Hidden`,
    );
    expect(oversized.sourceLimitExceeded).toBe(true);
    expect(oversized.blocks.some((block) => block.kind === "heading")).toBe(
      false,
    );
  });

  it("retains script and style as literal text across markdown lines", () => {
    const document = renderMarkdownPreview(
      "<script>\n\n# alert(1)\n\n</script>\n\n<style>\n\nbody{}\n\n</style>\n\nSafe",
    );
    expect(document.blocks.at(-1)).toEqual({ kind: "paragraph", text: "Safe" });
    expect(JSON.stringify(document)).toContain("alert(1)");
    expect(JSON.stringify(document)).toContain("body{}");
  });

  it("clamps a list as one visible block", () => {
    const first = "A".repeat(MAX_MARKDOWN_PREVIEW_BLOCK_CHARS - 2);
    const document = renderMarkdownPreview(`- ${first}\n- leftover`);
    expect(document.truncated).toBe(true);
    expect(document.blocks).toEqual([{ kind: "list", items: [first, "le"] }]);
  });

  it("clamps visible block text", () => {
    const document = renderMarkdownPreview(
      `# ${"T".repeat(MAX_MARKDOWN_PREVIEW_BLOCK_CHARS + 8)}`,
    );
    expect(document.blocks).toEqual([
      {
        kind: "heading",
        level: 1,
        text: "T".repeat(MAX_MARKDOWN_PREVIEW_BLOCK_CHARS),
      },
    ]);
    expect(document.truncated).toBe(true);
  });
});

describe("CommonMark block syntax", () => {
  it.each(["---", "***", "___", "* * *", "- - -", "   _ _ _ _", "  ----  "])(
    "renders %s as a rule",
    (rule) => {
      expect(renderMarkdownPreview(`Intro\n\n${rule}\n\nAfter`).blocks).toEqual(
        [
          { kind: "paragraph", text: "Intro" },
          { kind: "rule" },
          { kind: "paragraph", text: "After" },
        ],
      );
    },
  );
  it.each(["--", "**", "__", "-_*", "    ---", "--- text"])(
    "does not turn %s into a rule",
    (source) => {
      expect(
        renderMarkdownPreview(source).blocks.some(
          (block) => block.kind === "rule",
        ),
      ).toBe(false);
    },
  );
  it.each(["- - -", "* * *", "___"])("ends a list before %s", (rule) => {
    expect(
      renderMarkdownPreview(`- item\n${rule}\nAfter`).blocks.map(
        (block) => block.kind,
      ),
    ).toEqual(["list", "rule", "paragraph"]);
  });
  it("keeps table delimiter rows as tables", () => {
    expect(
      renderMarkdownPreview("| Name |\n| --- |\n| value |").blocks,
    ).toEqual([
      { kind: "table", align: [undefined], rows: [["Name"], ["value"]] },
    ]);
  });
  it.each([
    ["===", 1],
    ["---", 2],
    ["  ==  ", 1],
    ["-", 2],
  ])("uses %s as a setext underline", (underline, level) => {
    expect(
      renderMarkdownPreview(`A **title**\ncontinued\n${underline}\nAfter`)
        .blocks,
    ).toEqual([
      { kind: "heading", level, text: "A title continued" },
      { kind: "paragraph", text: "After" },
    ]);
  });
  it("lets spaced rules interrupt paragraphs without becoming headings", () => {
    expect(
      renderMarkdownPreview("Intro\n- - -\nAfter").blocks.map(
        (block) => block.kind,
      ),
    ).toEqual(["paragraph", "rule", "paragraph"]);
  });
  it("keeps tilde code literal until a matching fence of sufficient length", () => {
    expect(
      renderMarkdownPreview(
        "Before\n~~~~js\nconst a = *b*;\n```\n~~~\n~~~~ trailing\n~~~~~\nAfter",
      ).blocks,
    ).toEqual([
      { kind: "paragraph", text: "Before" },
      {
        kind: "code",
        language: "js",
        text: "const a = *b*;\n```\n~~~\n~~~~ trailing",
        fenceClosed: true,
      },
      { kind: "paragraph", text: "After" },
    ]);
  });
  it.each(["~~~", "```"])("reports an unclosed %s fence", (fence) => {
    expect(renderMarkdownPreview(`${fence}js\n*a*`).blocks).toEqual([
      { kind: "code", language: "js", text: "*a*", fenceClosed: false },
    ]);
  });
  it("joins blank and unspaced quote lines into one quote", () => {
    expect(renderMarkdownPreview("> one\n>\n> two\n>text").blocks).toEqual([
      { kind: "quote", text: "one two text" },
    ]);
  });
  it("counts rules toward the block cap", () => {
    const document = renderMarkdownPreview(
      "***\n".repeat(MAX_MARKDOWN_PREVIEW_BLOCKS) + "After",
    );
    expect(document.blocks).toEqual(
      Array.from({ length: MAX_MARKDOWN_PREVIEW_BLOCKS }, () => ({
        kind: "rule",
      })),
    );
    expect(document.truncated).toBe(true);
    expect(
      renderMarkdownPreview("***\n".repeat(MAX_MARKDOWN_PREVIEW_BLOCKS))
        .truncated,
    ).toBe(false);
  });
});
