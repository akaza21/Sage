import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("shared File and Diff document language", () => {
  it("is owned by both resource surfaces and loaded once by the app", async () => {
    const [
      entry,
      fileSurface,
      fileSurfaceContent,
      diffSurface,
      language,
      fileStyles,
      diffStyles,
      editorModeStyles,
    ] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/features/files/FileSurface.tsx"), "utf8"),
      readFile(resolve("src/features/files/FileSurfaceContent.tsx"), "utf8"),
      readFile(resolve("src/features/diff/DiffSurface.tsx"), "utf8"),
      readFile(resolve("src/styles/resource-document-language.css"), "utf8"),
      readFile(resolve("src/styles/file-editor-surface.css"), "utf8"),
      readFile(resolve("src/styles/diff-surface.css"), "utf8"),
      readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/resource-document-language\.css["'];/,
    );
    expect(fileSurface).toContain("<FileSurfaceContent");
    expect(
      fileSurfaceContent.match(/className="file-surface resource-document"/g),
    ).toHaveLength(2);
    expect(fileSurfaceContent).toContain(
      'className="resource-state resource-state--error"',
    );
    expect(diffSurface).toContain('className="diff-surface resource-document"');
    expect(language).toContain("@media (forced-colors: none)");
    expect(language).toMatch(
      /\.resource-document\s*\{[^}]*--resource-document-header-height:\s*36px;/s,
    );
    expect(language).toContain("--resource-document-code-foreground:");
    expect(language).toContain("--resource-document-gutter-foreground:");
    expect(language).toContain("--resource-document-gutter-divider:");
    expect(language).toContain("--resource-document-active-line:");
    expect(language).toContain("--resource-document-caret:");
    expect(language).toContain("--resource-document-selection:");
    expect(language).toMatch(
      /--resource-document-selection-foreground:\s*HighlightText;/,
    );

    expect(fileStyles).toContain("var(--resource-document-header-height)");
    expect(fileStyles).toContain("var(--resource-document-code-foreground)");
    expect(fileStyles).toContain("var(--resource-document-gutter-foreground)");
    expect(fileStyles).toContain("var(--resource-document-active-line)");
    expect(fileStyles).toContain("var(--resource-document-caret)");
    expect(fileStyles).toContain("var(--resource-document-selection)");
    expect(fileStyles).toContain(
      "var(--resource-document-selection-foreground)",
    );
    expect(fileStyles).toContain("container-type: inline-size");
    expect(fileStyles).toContain("@container (max-width: 560px)");
    expect(diffStyles).toContain("var(--resource-document-header-height)");
    expect(diffStyles).toContain("var(--resource-document-code-foreground)");
    expect(diffStyles).toContain("var(--resource-document-gutter-foreground)");
    expect(diffStyles).toContain("var(--resource-document-gutter-divider)");
    expect(diffStyles).toContain("var(--resource-document-selection)");
    expect(diffStyles).toContain(
      "var(--resource-document-selection-foreground)",
    );
    expect(editorModeStyles).not.toMatch(/\.workbench--editor \.file-/);
  });
});
