import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Editor visual-reference seam", () => {
  it("keeps portaled Editor surfaces on the Editor palette", async () => {
    const css = await readFile(
      resolve("src/styles/editor-visual-reference.css"),
      "utf8",
    );

    expect(css).toMatch(
      /\.workbench--editor,\s*\.new-file-dialog--editor,\s*\.dirty-dialog--editor,\s*\.menu-popover--editor\s*\{/,
    );
  });

  it("loads one Editor-scoped finish layer with the frozen neutral roles", async () => {
    const [entry, css, global] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/editor-visual-reference\.css["'];/,
    );
    expect(css).toContain("@media (forced-colors: none)");
    expect(css).toContain("--color-canvas: var(--doc-canvas)");
    expect(css).toContain("--color-plane-raised: var(--doc-plane-raised)");
    expect(css).toContain("--color-border-subtle: var(--doc-border-subtle)");
    expect(css).toContain("--color-file-script: var(--doc-file-script)");
    expect(css).toContain("--color-file-python: var(--doc-file-python)");
    expect(css).toContain("--color-file-rust: var(--doc-file-rust)");
    expect(css).toContain("--color-file-data: var(--doc-file-data)");
    expect(css).toContain("--color-file-media: var(--doc-file-media)");
    expect(css).toContain("--color-accent: var(--doc-accent)");
    expect(global).toMatch(/--doc-canvas\s*:\s*[^;]+;/);
    expect(global).toMatch(/--doc-plane-raised\s*:\s*#2a2a2a;/);
    expect(global).toMatch(/--doc-border-subtle\s*:\s*[^;]+;/);
    expect(global).toMatch(/--doc-file-script\s*:\s*#4c9df3;/);
    expect(global).toMatch(/--doc-file-python\s*:\s*#e2b66b;/);
    expect(global).toMatch(/--doc-file-rust\s*:\s*#ef9a9a;/);
    expect(global).toMatch(/--doc-file-data\s*:\s*#d7c078;/);
    expect(global).toMatch(/--doc-file-media\s*:\s*#75b4f7;/);
    expect(global).toMatch(/--doc-accent\s*:\s*[^;]+;/);
    expect(css).not.toMatch(/^:root\b/m);
  });

  it("keeps framed parts, resize affordances, and compact panes bounded", async () => {
    const [css, activityCss, resizeSource, workbenchSource, windowModeCss] =
      await Promise.all([
        readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
        readFile(resolve("src/styles/editor-activity-bar.css"), "utf8"),
        readFile(resolve("src/workbench/workbenchSidebarResize.ts"), "utf8"),
        readFile(resolve("src/workbench/Workbench.tsx"), "utf8"),
        readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
      ]);

    expect(activityCss).toMatch(
      /\.workbench--editor \.sidebar\s*\{[^}]*border:\s*1px solid var\(--color-border-subtle\);[^}]*border-radius:\s*var\(--radius-md\);/s,
    );
    expect(css).toMatch(
      /\.workbench--editor \.center-region\s*\{[^}]*border:\s*1px solid var\(--color-border-subtle\);[^}]*border-radius:\s*var\(--radius-md\);/s,
    );
    expect(css).toMatch(
      /\.center-region\[aria-hidden="true"\]\s*\{[^}]*margin-left:\s*min\(280px,\s*calc\(100% - 160px\)\);/s,
    );
    expect(resizeSource).toContain("EDITOR_ACTIVITY_BAR_WIDTH = 48");
    expect(workbenchSource).toContain(
      '"--editor-activity-width": `${EDITOR_ACTIVITY_BAR_WIDTH}px`',
    );
    expect(windowModeCss).toMatch(
      /calc\(\s*var\(--sidebar-width\)\s*\+\s*var\(--editor-activity-width\)\s*\)/,
    );
    expect(css).not.toContain("--editor-part-inset");
    expect(css).not.toContain("--editor-part-radius");
    expect(css).not.toContain("--editor-activity-width: 48px");
  });

  it("preserves Editor geometry and semantic typography in accessibility modes", async () => {
    const css = await readFile(
      resolve("src/styles/editor-visual-reference.css"),
      "utf8",
    );

    const forcedColorsNone = css.indexOf("@media (forced-colors: none)");
    const radiusProjection = css.indexOf("--radius-sm: 4px");
    expect(radiusProjection).toBeGreaterThan(-1);
    expect(radiusProjection).toBeLessThan(forcedColorsNone);
    expect(css).toContain("font-family: var(--font-family-ui);");
    expect(css).toMatch(
      /\.sidebar__heading > button\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;/s,
    );
    expect(css).toMatch(
      /\.workspace-search__details-toggle\s*\{[^}]*min-height:\s*28px;[^}]*border-radius:\s*var\(--radius-sm\);/s,
    );
  });

  it("gives secondary Editor tools explicit bounded states", async () => {
    const css = await readFile(
      resolve("src/styles/editor-visual-reference.css"),
      "utf8",
    );

    expect(css).toMatch(
      /\.workspace-search__result,[\s\S]*\.problem-card,[\s\S]*\.outline-card\s*\{[^}]*min-height:\s*28px;[^}]*border-radius:\s*var\(--radius-sm\);/s,
    );
    expect(css).toMatch(
      /\.workbench--editor \.status-bar\s*\{[^}]*border-top:\s*1px solid var\(--color-border-subtle\);/s,
    );
    expect(css).toContain(".workbench--editor .workspace-search__progress");
    expect(css).toContain(".workbench--editor .workspace-search__error");
    expect(css).toContain(".workbench--editor .sidebar__error");
    expect(css).toContain(".workbench--editor .problems-surface__empty");
    expect(css).toContain(".workbench--editor .outline-surface__empty");
  });
});
