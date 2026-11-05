import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Editor highlight language seam", () => {
  it("loads one normal-color Editor-owned highlight layer last", async () => {
    const [entry, css, visualCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/editor-highlight-language.css"), "utf8"),
      readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
    ]);

    expect(entry).toContain('import "./styles/editor-highlight-language.css"');
    expect(entry.indexOf("editor-highlight-language.css")).toBeGreaterThan(
      entry.indexOf("settings-surface.css"),
    );
    expect(css).toContain("@media (forced-colors: none)");
    expect(visualCss).toContain("--highlight-focus-edge: color-mix(");
    expect(visualCss).toContain("--highlight-focus-halo: color-mix(");
    expect(visualCss).toContain(
      "--editor-focus-edge: var(--highlight-focus-edge)",
    );
    expect(visualCss).toContain(
      "--editor-focus-halo: var(--highlight-focus-halo)",
    );
    expect(visualCss).toContain(
      "--editor-selection-fill: var(--highlight-selection-fill)",
    );
    expect(visualCss).toContain(
      "--editor-selection-edge: var(--highlight-selection-edge)",
    );
    expect(visualCss).toContain(
      "--editor-active-fill: var(--highlight-active-fill)",
    );
    expect(visualCss).toContain(
      "--editor-active-edge: var(--highlight-active-edge)",
    );
    expect(visualCss).toContain(
      "--editor-focus-halo-shadow: var(--highlight-focus-halo-shadow)",
    );
    expect(visualCss).toContain(
      "--editor-focus-frame-shadow: var(--highlight-focus-frame-shadow)",
    );
    expect(visualCss).toContain(
      "--focus-ring: var(--focus-ring-width) solid var(--highlight-focus-edge)",
    );
    expect(visualCss).toContain(
      "--focus-ring-inset: inset 0 0 0 var(--focus-ring-width)",
    );
    expect(visualCss).toContain("--editor-code-selection:");
    expect(css).not.toContain("--focus-ring-width:");
    expect(css).not.toContain("--focus-ring-offset:");
    expect(css).not.toMatch(/@keyframes|animation:/);
    expect(css).not.toMatch(/(?:#fff(?:fff)?\b|\bwhite\b)/i);
    expect(css).not.toContain(".workbench--agent");
    expect(css).not.toMatch(/^:root\b/m);
    expect(css).not.toContain("--color-diff-add");
    expect(css).not.toContain("--color-diff-delete");
  });

  it("projects the highlight roles across every Editor control family", async () => {
    // The activity rail and its splitter are read from computed styles in
    // tests/ui/sidebar-rail-states.visual.ts; row selection, search matches
    // and count badges in tests/ui/editor-highlight-roles.visual.ts.
    const [css, visualCss] = await Promise.all([
      readFile(resolve("src/styles/editor-highlight-language.css"), "utf8"),
      readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
    ]);

    for (const selector of [
      ".workspace-search__input:focus-within",
      ".workspace-search__filters input:focus-visible",
      ".workspace-search__query-actions",
      'button[aria-pressed="true"]',
      ".title-bar__mode--active",
      ".tab--active",
      ".explorer-action-menu__item",
      ".outline-card",
      ".repository-commit__label",
      "textarea:focus-visible",
      ".editor-input .cm-editor.cm-focused",
      ".editor-input .cm-content:focus-visible",
      ".editor-find__query input:focus-visible",
      '.settings-surface input[type="search"]:focus-visible',
      ".settings-surface select:focus-visible",
    ]) {
      expect(css).toContain(selector);
    }
    expect(css).toMatch(
      /\.workbench--editor\s+\.source-control-selector\s+\.searchable-picker__trigger:focus-visible/,
    );
    expect(css).toContain(
      ".menu-popover--editor .searchable-picker__input:focus-visible",
    );
    expect(visualCss).toContain(".new-file-dialog--editor");
    expect(visualCss).toContain(".dirty-dialog--editor");
  });
});
