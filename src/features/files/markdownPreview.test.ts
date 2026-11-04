import { describe, expect, it, vi } from "vitest";

import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  MAX_CHAT_FILE_LINKS,
  MAX_CHAT_FILE_LINK_PATH_BYTES,
  MAX_MARKDOWN_LIST_DEPTH,
  renderMarkdownPreview,
} from "./markdownPreview";
import { canPreviewDiagram, canPreviewMarkdown } from "./filePreviewKind";

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

  it("keeps excess same-line nesting literal at the container depth limit", () => {
    const document = renderMarkdownPreview("- ".repeat(10000) + "leaf");
    expect(document.truncated).toBe(true);
    const block = document.blocks[0];
    expect(block.kind).toBe("list");
    if (block.kind !== "list") throw new Error("Expected a list");
    expect(block.items).toEqual([
      ...Array(MAX_MARKDOWN_LIST_DEPTH - 1).fill(""),
      "- ".repeat(MAX_MARKDOWN_PREVIEW_BLOCK_CHARS / 2),
    ]);
    expect(block.shape?.at(-1)?.depth).toBe(MAX_MARKDOWN_LIST_DEPTH - 1);
  });

  it("uses absolute tab stops for same-line nested list markers", () => {
    expect(
      renderMarkdownPreview("- -\tchild\n    1. grandchild").blocks,
    ).toEqual([
      {
        kind: "list",
        items: ["", "child", "grandchild"],
        shape: [
          { depth: 0, ordered: false },
          { depth: 1, ordered: false },
          { depth: 2, ordered: true },
        ],
      },
    ]);
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
      expect(renderMarkdownPreview(source).blocks).toEqual([
        { kind: "paragraph", text: source.trim() },
      ]);
    },
  );
  it.each(["- - -", "* * *", "___"])("ends a list before %s", (rule) => {
    expect(renderMarkdownPreview(`- item\n${rule}\nAfter`).blocks).toEqual([
      { kind: "list", items: ["item"] },
      { kind: "rule" },
      { kind: "paragraph", text: "After" },
    ]);
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
    expect(renderMarkdownPreview("Intro\n- - -\nAfter").blocks).toEqual([
      { kind: "paragraph", text: "Intro" },
      { kind: "rule" },
      { kind: "paragraph", text: "After" },
    ]);
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

describe("fence indentation and empty list precedence", () => {
  it.each(["~~~", "```"])(
    "keeps a four-space %s closer inside literal code",
    (fence) => {
      expect(
        renderMarkdownPreview(`${fence}js\ncode\n    ${fence}\nAfter`).blocks,
      ).toEqual([
        {
          kind: "code",
          language: "js",
          text: `code\n    ${fence}\nAfter`,
          fenceClosed: false,
        },
      ]);
    },
  );
  it.each(["~~~", "```"])(
    "admits a three-space %s opener and closer",
    (fence) => {
      expect(
        renderMarkdownPreview(
          `Before\n   ${fence}js\n   code\n   ${fence}\nAfter`,
        ).blocks,
      ).toEqual([
        { kind: "paragraph", text: "Before" },
        { kind: "code", language: "js", text: "code", fenceClosed: true },
        { kind: "paragraph", text: "After" },
      ]);
    },
  );
  it.each(["~~~", "```"])(
    "does not open a four-space %s fence or interrupt a paragraph",
    (fence) => {
      expect(renderMarkdownPreview(`    ${fence}js\ncode`).blocks).toEqual([
        { kind: "paragraph", text: `${fence}js code` },
      ]);
      expect(
        renderMarkdownPreview(`Before\n    ${fence}js\ncode`).blocks,
      ).toEqual([{ kind: "paragraph", text: `Before ${fence}js code` }]);
    },
  );
  it.each(["-", "+", "*", "-   ", "+\t"])(
    "classifies empty %s as a list before a rule",
    (marker) => {
      expect(renderMarkdownPreview(`${marker}\n---`).blocks).toEqual([
        { kind: "list", items: [""] },
        { kind: "rule" },
      ]);
    },
  );
  it.each(["1.", "2)"])(
    "retains empty ordered marker %s before a rule",
    (marker) => {
      expect(renderMarkdownPreview(`${marker}\n---`).blocks).toEqual([
        { kind: "list", items: [""], ordered: true, start: Number(marker[0]) },
        { kind: "rule" },
      ]);
    },
  );
  it("keeps empty items among nonempty items without converting them to headings", () => {
    expect(renderMarkdownPreview("- one\n-\n- two\n---").blocks).toEqual([
      { kind: "list", items: ["one", "", "two"] },
      { kind: "rule" },
    ]);
  });
  it("does not use an equals underline to turn an empty list into a heading", () => {
    expect(renderMarkdownPreview("-\n===").blocks).toEqual([
      { kind: "list", items: [""] },
      { kind: "paragraph", text: "===" },
    ]);
  });
  it("bounds retained empty items just like one-character items", () => {
    const document = renderMarkdownPreview(
      "-\n".repeat(MAX_MARKDOWN_PREVIEW_BLOCK_CHARS + 1),
    );
    expect(document.blocks).toEqual([
      { kind: "list", items: Array(MAX_MARKDOWN_PREVIEW_BLOCK_CHARS).fill("") },
    ]);
    expect(document.truncated).toBe(true);
  });
});

describe("ATX classification before setext", () => {
  it.each([
    ["#", 1, ""],
    [" #", 1, ""],
    ["  ##  ", 2, ""],
    ["   ######\t", 6, ""],
    [" # Title", 1, "Title"],
    ["  ## Title", 2, "Title"],
    ["   # Title", 1, "Title"],
    ["###\tTitle", 3, "Title"],
  ])("classifies %s before a following rule", (source, level, text) => {
    expect(renderMarkdownPreview(`${source}\n---`).blocks).toEqual([
      { kind: "heading", level, text },
      { kind: "rule" },
    ]);
    expect(renderMarkdownPreview(`Intro\n${source}\n---`).blocks).toEqual([
      { kind: "paragraph", text: "Intro" },
      { kind: "heading", level, text },
      { kind: "rule" },
    ]);
  });
  it.each(["####### Title", "#Title", "    # Title"])(
    "keeps non-ATX %s as paragraph text eligible for setext",
    (source) => {
      expect(renderMarkdownPreview(`${source}\n---`).blocks).toEqual([
        { kind: "heading", level: 2, text: source.trim() },
      ]);
    },
  );
});

describe("shared list interruption classification", () => {
  it("keeps a quote paragraph open across marked indented continuation", () => {
    expect(renderMarkdownPreview("> foo\n>     bar\nbaz\n").blocks).toEqual([
      { kind: "quote", text: "foo bar baz" },
    ]);
  });

  it.each([
    [">     foo\n    bar\n", "foo", "bar"],
    ["> ```\nfoo\n```\n", "```", "foo"],
    ["> ```\n> code\noutside\n", "``` code", "outside"],
  ])(
    "does not lazily continue a quote opened by code: %j",
    (source, text, outside) => {
      expect(renderMarkdownPreview(source).blocks.slice(0, 2)).toEqual([
        { kind: "quote", text },
        { kind: "paragraph", text: outside },
      ]);
    },
  );

  it("does not collect a fenced list child's code as paragraph continuation", () => {
    expect(renderMarkdownPreview("- ```\n  code\n").blocks).toEqual([
      { kind: "list", items: ["```"] },
      { kind: "paragraph", text: "code" },
    ]);
  });

  it.each(["- one", "-\n  one"])(
    "keeps an empty nested marker in the paragraph opened by %j",
    (prefix) => {
      expect(renderMarkdownPreview(`${prefix}\n  +\n  two`).blocks).toEqual([
        { kind: "list", items: ["one + two"] },
      ]);
    },
  );

  it("admits an empty nested list before a paragraph has opened", () => {
    expect(renderMarkdownPreview("-\n  +").blocks).toEqual([
      {
        kind: "list",
        items: ["", ""],
        shape: [
          { depth: 0, ordered: false },
          { depth: 1, ordered: false },
        ],
      },
    ]);
  });

  it.each(["-    ", "-\t"])(
    "uses the current sibling's content column after %j",
    (prefix) => {
      expect(
        renderMarkdownPreview(`${prefix}parent\n- sibling\n  1. child`).blocks,
      ).toEqual([
        {
          kind: "list",
          items: ["parent", "sibling", "child"],
          shape: [
            { depth: 0, ordered: false },
            { depth: 0, ordered: false },
            { depth: 1, ordered: true },
          ],
        },
      ]);
    },
  );
  it.each(["~~~", "```"])(
    "reads %s indentation relative to a two-digit list item's content",
    (fence) => {
      expect(
        renderMarkdownPreview(
          `10. example\n    ${fence}js\n    const x = 1;\n    ${fence}`,
        ).blocks,
      ).toEqual([
        { kind: "list", ordered: true, start: 10, items: ["example"] },
        {
          kind: "code",
          language: "js",
          text: "const x = 1;",
          fenceClosed: true,
        },
      ]);
    },
  );
  it.each(["-\t", "+\t", "*\t", " -\t", "  -\t", "- \t", "-   "])(
    "keeps a two-space item beside a parent separated by %j",
    (prefix) => {
      expect(
        renderMarkdownPreview(`${prefix}parent\n  - sibling`).blocks,
      ).toEqual(
        prefix.trim() === "+" || prefix.trim() === "*"
          ? [
              { kind: "list", items: ["parent"] },
              { kind: "list", items: ["sibling"] },
            ]
          : [{ kind: "list", items: ["parent", "sibling"] }],
      );
    },
  );

  it.each([
    ["-\t", 4, false],
    [" -\t", 4, false],
    ["  -\t", 4, false],
    ["   -\t", 8, false],
    ["1.\t", 4, true],
    ["12)\t", 4, true],
    ["123.\t", 8, true],
    [" 1.\t", 4, true],
    ["-\t ", 5, false],
  ] as const)(
    "uses %j to nest at column %i and keeps the preceding column as a sibling",
    (prefix, column, ordered) => {
      const source = [
        `${prefix}parent`,
        `${" ".repeat(column)}- child`,
        `${" ".repeat(column - 1)}${ordered ? "2." : "-"} sibling`,
      ].join("\n");
      const changesDelimiter = prefix.includes(")");
      expect(renderMarkdownPreview(source).blocks).toEqual([
        {
          kind: "list",
          items: changesDelimiter
            ? ["parent", "child"]
            : ["parent", "child", "sibling"],
          ...(ordered ? { ordered: true, start: Number.parseInt(prefix) } : {}),
          shape: [
            { depth: 0, ordered },
            { depth: 1, ordered: false },
            ...(!changesDelimiter ? [{ depth: 0, ordered }] : []),
          ],
        },
        ...(changesDelimiter
          ? [{ kind: "list", items: ["sibling"], ordered: true, start: 2 }]
          : []),
      ]);
    },
  );

  it("aligns mixed space and tab indentation at the same nesting level", () => {
    expect(
      renderMarkdownPreview("- parent\n \t- child\n    - sibling").blocks,
    ).toEqual([
      {
        kind: "list",
        items: ["parent", "child", "sibling"],
        shape: [
          { depth: 0, ordered: false },
          { depth: 1, ordered: false },
          { depth: 1, ordered: false },
        ],
      },
    ]);
  });

  it("keeps the default content indentation after an empty tab-separated item", () => {
    expect(renderMarkdownPreview("- one\n-\t\n  - child").blocks).toEqual([
      {
        kind: "list",
        items: ["one", "", "child"],
        shape: [
          { depth: 0, ordered: false },
          { depth: 0, ordered: false },
          { depth: 1, ordered: false },
        ],
      },
    ]);
  });

  it.each(["-     ", "-\t\t", "- \t\t"])(
    "uses one column of list padding when %j separates more than four columns",
    (prefix) => {
      expect(
        renderMarkdownPreview(`${prefix}parent\n  - child`).blocks,
      ).toEqual([
        {
          kind: "list",
          items: ["parent", "child"],
          shape: [
            { depth: 0, ordered: false },
            { depth: 1, ordered: false },
          ],
        },
      ]);
    },
  );

  it.each(["-\tx", "   -\tx"])(
    "interrupts a paragraph with short tab-separated content: %j",
    (line) => {
      expect(renderMarkdownPreview(`Intro\n${line}`).blocks).toEqual([
        { kind: "paragraph", text: "Intro" },
        { kind: "list", items: ["x"] },
      ]);
    },
  );

  it.each(["-", "+", "*", "1."])(
    "interrupts a paragraph at a tab-separated %s item",
    (marker) => {
      const ordered = /^\d/u.test(marker);
      expect(
        renderMarkdownPreview(`Intro\n${marker}\titem\n---`).blocks,
      ).toEqual([
        { kind: "paragraph", text: "Intro" },
        {
          kind: "list",
          items: ["item"],
          ...(ordered ? { ordered: true, start: Number(marker[0]) } : {}),
        },
        { kind: "rule" },
      ]);
    },
  );
  it.each(["- item", "   -\titem", "1. item"])(
    "preserves admitted indentation and ordered starts for %s",
    (line) => {
      expect(renderMarkdownPreview(`Intro\n${line}\n---`).blocks).toEqual([
        { kind: "paragraph", text: "Intro" },
        {
          kind: "list",
          items: ["item"],
          ...(line.startsWith("1") ? { ordered: true, start: 1 } : {}),
        },
        { kind: "rule" },
      ]);
    },
  );
  it.each([
    "+",
    "2.",
    "2)\titem",
    "2. item",
    "    -\titem",
    "\t- item",
    "1234567890. item",
    "-item",
  ])("keeps %s inside the paragraph", (line) => {
    expect(renderMarkdownPreview(`Intro\n${line}\n---`).blocks).toEqual([
      {
        kind: "heading",
        level: 2,
        text: `Intro ${line.trim().replace(/\s+/gu, " ")}`,
      },
    ]);
  });
  it.each(["+ ", "+\t", "* \t"])(
    "keeps separator-bearing empty markers inside a setext heading: %s",
    (line) => {
      expect(renderMarkdownPreview(`Intro\n${line}\n---`).blocks).toEqual([
        { kind: "heading", level: 2, text: `Intro ${line.trim()}` },
      ]);
    },
  );
  it("keeps setext precedence over a marker with trailing whitespace", () => {
    expect(renderMarkdownPreview("Intro\n-\t\n---").blocks).toEqual([
      { kind: "heading", level: 2, text: "Intro" },
      { kind: "rule" },
    ]);
  });
});
