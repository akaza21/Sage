import { describe, expect, it } from "vitest";
import { renderMarkdownPreview } from "./markdownPreview";
import { renderMarkdownStream } from "./markdownStream";

describe("streamed markdown document", () => {
  it("shares the component cap with a frozen quote while appending rich blocks", () => {
    const source = "> ```python\n> ```\n>\n".repeat(60) + "\n# Kept\n\nTail";
    const before = renderMarkdownStream(source);
    const appended = source + "\n\n```python\n```\n\n# Skipped";
    const after = renderMarkdownStream(appended, before);
    expect(after.document).toEqual(renderMarkdownPreview(appended));
    expect(after.document.blocks.map((block) => block.kind)).toEqual([
      "quote",
      "heading",
      "paragraph",
      "code",
    ]);
    expect(after.document.truncated).toBe(true);
  });

  it.each(["- x\n", "> x\n"])(
    "keeps a huge single container (%j) truncated across streaming updates",
    (line) => {
      const source = line.repeat(65535);
      const before = renderMarkdownStream(source);
      const appended = source + line;
      const after = renderMarkdownStream(appended, before);
      expect(before.document.truncated).toBe(true);
      expect(after.document.truncated).toBe(true);
      expect(after.document.blocks).toHaveLength(1);
      expect(after.document).toEqual(renderMarkdownPreview(appended));
    },
  );

  it("bounds a huge container after a frozen streaming prefix", () => {
    const source = "# Kept\n\n# Kept too\n\nTail\n\n- x\n";
    const before = renderMarkdownStream(source);
    const appended = source + "- x\n".repeat(65536);
    const after = renderMarkdownStream(appended, before);
    expect(after.document.blocks[0]).toBe(before.document.blocks[0]);
    expect(after.document.truncated).toBe(true);
    expect(after.document).toEqual(renderMarkdownPreview(appended));
  });

  it("bounds a large appended suffix by the allowance left after frozen blocks", () => {
    const source = "# x\n".repeat(64);
    const before = renderMarkdownStream(source);
    const appended = source + "# **skipped** [link](tail.ts)\n".repeat(5000);
    const after = renderMarkdownStream(appended, before);
    expect(after.document.blocks[0]).toBe(before.document.blocks[0]);
    expect(after.document.blocks).toHaveLength(64);
    expect(after.document.fileLinks).toEqual([]);
    expect(after.document.truncated).toBe(true);
    expect(after.document).toEqual(renderMarkdownPreview(appended));
  });

  it("preserves newline tokens in unfinished code and removes only the consumed closer separator", () => {
    let source = "# Kept\n\np\n\nq\n\n~~~js\na";
    let state = renderMarkdownStream(source);
    for (const text of ["a\n", "a\n\n"]) {
      source += "\n";
      state = renderMarkdownStream(source, state);
      expect(state.document.blocks.at(-1)).toEqual({
        kind: "code",
        language: "js",
        text,
        fenceClosed: false,
      });
    }
    state = renderMarkdownStream(source + "~~~", state);
    expect(state.document.blocks.at(-1)).toEqual({
      kind: "code",
      language: "js",
      text: "a\n",
      fenceClosed: true,
    });
  });
  it("keeps overflow from frozen blocks visible while appending to the tail", () => {
    const source = "# " + "x".repeat(5000) + "\n\np\n\nq\n\nTail";
    const before = renderMarkdownStream(source);
    const after = renderMarkdownStream(source + " more", before);
    expect(after.document.truncated).toBe(true);
    expect(after.document).toEqual(renderMarkdownPreview(source + " more"));
  });
  it("preserves closed blocks while an open paragraph becomes a heading, table, list or fence", () => {
    const prefix = "# Kept\n\n[readme](README.md)\n\n***\n\n";
    for (const tail of [
      "Title\n---",
      "| H |\n| - |\n| body |",
      "- [ ] first\n  - second",
      "```mermaid\nA --> B\n```",
      "> one\n> two",
    ]) {
      let state = renderMarkdownStream(prefix);
      for (const character of tail)
        state = renderMarkdownStream(state.source + character, state);
      expect(state.document).toEqual(renderMarkdownPreview(prefix + tail));
    }
  });
  it("reuses the frozen prefix and rebases file links without reparsing previous answers", () => {
    const source = "# Kept\n\n[first](a.ts)\n\n***\n\nTail";
    const before = renderMarkdownStream(source);
    const after = renderMarkdownStream(source + " [second](b.ts)", before);
    expect(after.document.blocks[0]).toBe(before.document.blocks[0]);
    expect(after.document).toEqual(
      renderMarkdownPreview(source + " [second](b.ts)"),
    );
    expect(after.document.fileLinks.map((link) => link.destination)).toEqual([
      "a.ts",
      "b.ts",
    ]);
  });
  it("invalidates the suffix on replacements and bounds retained source and blocks", () => {
    const before = renderMarkdownStream("old\n\ntail");
    expect(renderMarkdownStream("new", before).document).toEqual(
      renderMarkdownPreview("new"),
    );
    const source = "# H\n\n".repeat(70);
    const after = renderMarkdownStream(
      source,
      renderMarkdownStream(source.slice(0, -1)),
    );
    expect(after.document).toEqual(renderMarkdownPreview(source));
    expect(after.document.blocks).toHaveLength(64);
    expect(renderMarkdownStream("x".repeat(300000)).source.length).toBe(262144);
  });
});
