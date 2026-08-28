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
  });

  it("uses UI tokens without overriding the editor appearance font size", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/file-editor-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.file-header\s*\{[^}]*font-size:\s*var\(--type-size-caption\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.editor-notice\s*\{[^}]*font-size:\s*var\(--type-size-editor-ui\);/s,
    );
    expect(surfaceCss).not.toMatch(
      /\.editor-input \.cm-scroller\s*\{[^}]*font-size:/s,
    );
  });
});
