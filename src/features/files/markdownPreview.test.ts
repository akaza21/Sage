import { describe, expect, it } from "vitest";

import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  MAX_CHAT_FILE_LINKS,
  MAX_CHAT_FILE_LINK_PATH_BYTES,
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
  it("does not retain empty sanitized list items or their formatting records", () => {
    const document = renderMarkdownPreview(
      "- <div>\n".repeat(10000) + "- **Retained**",
    );
    expect(document.blocks).toEqual([{ kind: "list", items: ["Retained"] }]);
    expect(document.formats).toHaveLength(1);
    expect(document.formats[0].itemIndex).toBe(0);
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
  it("retains internal CRLF and exact block/source limits", () => {
    expect(
      renderMarkdownPreview(
        "```html\r\n<script>run()</script>\r\nline  \r\n```\r\n",
      ).blocks[0],
    ).toMatchObject({ text: "<script>run()</script>\r\nline  " });
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
      { kind: "paragraph", text: "Hello world and docs." },
      { kind: "list", items: ["one", "two"] },
      { kind: "quote", text: "quoted" },
      {
        kind: "code",
        text: "code <em>line</em>",
        language: "",
        fenceClosed: true,
      },
      { kind: "paragraph", text: "diagram" },
    ]);
    expect(JSON.stringify(document)).not.toContain("https://example.test");
    expect(JSON.stringify(document)).not.toContain("alert(1)");
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

  it("drops script and style that span markdown lines", () => {
    const document = renderMarkdownPreview(
      "<script>\n\n# alert(1)\n\n</script>\n\n<style>\n\nbody{}\n\n</style>\n\nSafe",
    );
    expect(document.blocks).toEqual([{ kind: "paragraph", text: "Safe" }]);
    expect(JSON.stringify(document)).not.toContain("alert(1)");
    expect(JSON.stringify(document)).not.toContain("body{}");
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
