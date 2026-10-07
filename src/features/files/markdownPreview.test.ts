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
