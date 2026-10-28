import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("File editor surface stylesheet seam", () => {
  it("loads file editor rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/file-editor-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/file-editor-surface\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.file-surface\b/m);
    expect(shellCss).not.toMatch(/^\.editor-input\b/m);
    expect(surfaceCss).toMatch(/^\.file-surface\b/m);
    expect(surfaceCss).toMatch(/^\.editor-input\b/m);
    expect(surfaceCss).toMatch(
      /\.file-editor-stage\s*\{[^}]*position:\s*relative;[^}]*overflow:\s*hidden;/s,
    );
    expect(surfaceCss).toMatch(
      /\.editor-find--overlay\s*\{[^}]*position:\s*absolute;[^}]*right:\s*12px;[^}]*width:\s*min\(440px, calc\(100% - 24px\)\);/s,
    );
    expect(surfaceCss).toMatch(
      /@container \(max-width: 420px\)\s*\{[\s\S]*\.editor-find--overlay\s*\{[^}]*grid-template-columns:\s*20px minmax\(0, 1fr\) repeat\(3, 24px\);[\s\S]*\.editor-find--overlay > \.editor-find__field\s*\{[^}]*grid-column:\s*2 \/ -1;/,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors: active\)\s*\{\s*\.editor-input \.cm-focused\s*\{[^}]*outline:\s*var\(--focus-ring\);[^}]*box-shadow:\s*none;[^}]*\}[\s\S]*\.editor-find--overlay\s*\{[^}]*border-color:\s*CanvasText;[^}]*box-shadow:\s*none;/s,
    );
  });

  it("uses UI tokens without overriding the editor appearance font size", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/file-editor-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.file-header\s*\{[^}]*font-size:\s*var\(--font-size-xs\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.editor-notice\s*\{[^}]*font-size:\s*var\(--font-size-sm\);/s,
    );
    expect(surfaceCss).not.toMatch(
      /\.editor-input \.cm-scroller\s*\{[^}]*font-size:/s,
    );
  });
});
