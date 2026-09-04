import { readdir, readFile } from "node:fs/promises";
import { join, relative, resolve } from "node:path";

import { describe, expect, it } from "vitest";

const MAX_SOURCE_ENTRIES = 20_000;
const MAX_SOURCE_STYLESHEETS = 500;

async function readSourceStylesheets() {
  const sourceRoot = resolve("src");
  const pendingDirectories = [sourceRoot];
  const stylesheets: Array<{ path: string; source: string }> = [];
  let visitedEntries = 0;

  while (pendingDirectories.length > 0) {
    const directory = pendingDirectories.pop();
    if (directory == null) break;
    const entries = await readdir(directory, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      visitedEntries += 1;
      if (visitedEntries > MAX_SOURCE_ENTRIES) {
        throw new Error(
          `Source stylesheet inventory exceeded ${MAX_SOURCE_ENTRIES} entries`,
        );
      }
      const absolutePath = join(directory, entry.name);
      if (entry.isDirectory()) {
        pendingDirectories.push(absolutePath);
      } else if (entry.isFile() && entry.name.endsWith(".css")) {
        if (stylesheets.length >= MAX_SOURCE_STYLESHEETS) {
          throw new Error(
            `Source stylesheet inventory exceeded ${MAX_SOURCE_STYLESHEETS} CSS files`,
          );
        }
        stylesheets.push({
          path: relative(sourceRoot, absolutePath),
          source: await readFile(absolutePath, "utf8"),
        });
      }
    }
  }

  return stylesheets.sort((left, right) => left.path.localeCompare(right.path));
}

describe("document token authority", () => {
  const REQUIRED_DOC_TOKENS = [
    "--doc-canvas",
    "--doc-panel",
    "--doc-surface",
    "--doc-surface-hover",
    "--doc-surface-active",
    "--doc-surface-selected",
    "--doc-border-subtle",
    "--doc-border-strong",
    "--doc-text-primary",
    "--doc-text-secondary",
    "--doc-text-muted",
    "--doc-text-disabled",
    "--doc-text-inverse",
    "--doc-accent",
    "--doc-accent-hover",
    "--doc-accent-muted",
    "--doc-accent-foreground",
    "--doc-focus-ring",
    "--doc-selection",
    "--doc-elevation-popover",
    "--doc-elevation-floating",
    "--doc-code-foreground",
    "--doc-gutter-foreground",
    "--doc-gutter-divider",
    "--doc-active-line",
    "--doc-caret",
    "--doc-selection-background",
    "--doc-selection-foreground",
    "--doc-error-foreground",
    "--doc-syntax-meta",
    "--doc-syntax-keyword",
    "--doc-syntax-atom",
    "--doc-syntax-literal",
    "--doc-syntax-string",
    "--doc-syntax-regexp",
    "--doc-syntax-variable-definition",
    "--doc-syntax-variable-local",
    "--doc-syntax-type",
    "--doc-syntax-class",
    "--doc-syntax-variable-special",
    "--doc-syntax-property-definition",
    "--doc-syntax-comment",
    "--doc-syntax-invalid",
  ];
  const REQUIRED_DIFF_TOKENS = [
    "--diff-add",
    "--diff-add-muted",
    "--diff-delete",
    "--diff-delete-muted",
  ];

  it("declares every document and diff token once in global.css", async () => {
    const global = await readFile(resolve("src/styles/global.css"), "utf8");
    const [beforeForced, forcedBlock = ""] = global.split(
      "@media (forced-colors: active)",
    );
    for (const token of [...REQUIRED_DOC_TOKENS, ...REQUIRED_DIFF_TOKENS]) {
      const rootMatches =
        beforeForced.match(new RegExp(`${token}\\s*:`, "g")) ?? [];
      expect(rootMatches, token).toHaveLength(1);
      // Every token carries a value in the authority. Exact hues live in
      // global.css alone; preservation is proven by the byte-identical
      // file/diff baselines plus live cross-mode computed equality.
      const value = beforeForced
        .match(new RegExp(`${token}\\s*:\\s*([^;]+);`))?.[1]
        ?.trim();
      expect(value, token).toBeTruthy();
      if (token.startsWith("--doc-syntax-")) {
        expect(forcedBlock).not.toContain(`${token}:`);
      } else {
        const forcedMatches =
          forcedBlock.match(new RegExp(`${token}\\s*:`, "g")) ?? [];
        expect(forcedMatches, token).toHaveLength(1);
      }
    }
    expect(global).toMatch(/--diff-add-muted:\s*color-mix\(/s);
    expect(global).toMatch(/--diff-delete-muted:\s*color-mix\(/s);
  });

  it("maps both consumers to the token authority without raw colours", async () => {
    const [language, editorModeStyles] = await Promise.all([
      readFile(resolve("src/styles/resource-document-language.css"), "utf8"),
      readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
    ]);
    const rawHexRgb = /#[0-9a-fA-F]{3,8}\b|\brgb\s*\(/;
    expect(language).not.toMatch(rawHexRgb);
    expect(editorModeStyles).not.toMatch(rawHexRgb);
    for (const token of REQUIRED_DOC_TOKENS) {
      if (token.startsWith("--doc-syntax-")) continue;
      if (
        token === "--doc-elevation-popover" ||
        token === "--doc-elevation-floating" ||
        token === "--doc-text-inverse" ||
        token === "--doc-accent-foreground"
      ) {
        continue;
      }
      expect(language, token).toContain(`var(${token})`);
    }
    for (const token of REQUIRED_DIFF_TOKENS) {
      expect(language, token).toContain(`var(${token})`);
    }
    for (const token of [
      "--doc-canvas",
      "--doc-panel",
      "--doc-surface",
      "--doc-surface-hover",
      "--doc-surface-active",
      "--doc-surface-selected",
      "--doc-border-subtle",
      "--doc-border-strong",
      "--doc-text-primary",
      "--doc-text-secondary",
      "--doc-text-muted",
      "--doc-text-disabled",
      "--doc-text-inverse",
      "--doc-accent",
      "--doc-accent-hover",
      "--doc-accent-muted",
      "--doc-accent-foreground",
      "--doc-focus-ring",
      "--doc-selection",
      "--doc-elevation-popover",
      "--doc-elevation-floating",
    ]) {
      expect(editorModeStyles, token).toContain(`var(${token})`);
    }
    expect(language).toContain("--color-diff-add: var(--diff-add)");
    expect(language).toContain("--color-diff-delete: var(--diff-delete)");
  });

  it("drives document syntax colors from tokens in the editor runtime", async () => {
    const [language, runtime] = await Promise.all([
      readFile(resolve("src/styles/resource-document-language.css"), "utf8"),
      readFile(
        resolve("src/features/files/CodeMirrorEditorRuntime.ts"),
        "utf8",
      ),
    ]);
    expect(runtime).toContain("HighlightStyle.define(");
    expect(runtime).toContain("defaultHighlightStyle.specs");
    expect(runtime).toContain("DOCUMENT_SYNTAX_TOKENS_BY_SPEC_INDEX");
    expect(runtime).toContain("DOCUMENT_SYNTAX_SPEC_FINGERPRINT");
    expect(runtime).toContain("refusing to mis-map token colors");
    expect(runtime).toContain(
      "syntaxHighlighting(documentSyntaxHighlightStyle, { fallback: true })",
    );
    expect(runtime).not.toContain("syntaxHighlighting(defaultHighlightStyle");
    expect(runtime).not.toContain("@lezer/highlight");
    expect(runtime).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    for (const token of REQUIRED_DOC_TOKENS) {
      if (!token.startsWith("--doc-syntax-")) continue;
      expect(runtime, token).toContain(`var(${token})`);
    }
    expect(language).not.toContain(".tok-");
  });

  it("has no agent-mode document or diff token override in any source stylesheet", async () => {
    const stylesheets = await readSourceStylesheets();
    expect(stylesheets.length).toBeGreaterThan(0);
    for (const stylesheet of stylesheets) {
      const agentBlocks =
        stylesheet.source.match(/\.workbench--agent[^{]*\{[^}]*\}/gs) ?? [];
      for (const block of agentBlocks) {
        expect(block, stylesheet.path).not.toMatch(/--(?:doc|diff)-[\w-]+\s*:/);
      }
    }
  });
});

describe("shared File and Diff document language", () => {
  it("is owned by both resource surfaces and loaded once by the app", async () => {
    const [
      entry,
      fileSurface,
      fileSurfaceContent,
      diffSurface,
      resourceStateNotice,
      language,
      fileStyles,
      diffStyles,
      editorModeStyles,
    ] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/features/files/FileSurface.tsx"), "utf8"),
      readFile(resolve("src/features/files/FileSurfaceContent.tsx"), "utf8"),
      readFile(resolve("src/features/diff/DiffSurface.tsx"), "utf8"),
      readFile(resolve("src/ui/ResourceStateNotice.tsx"), "utf8"),
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
    expect(fileSurfaceContent).toContain("<ResourceStateNotice");
    expect(diffSurface).toContain("<ResourceStateNotice");
    expect(resourceStateNotice).toContain(
      "className={`resource-state resource-state--${state}`}",
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
    expect(language).toContain("--resource-document-selection-foreground:");
    expect(language).toContain("var(--doc-selection-foreground)");

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
