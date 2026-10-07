import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Editor Activity Bar stylesheet seam", () => {
  it("loads Activity Bar rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/editor-activity-bar.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/editor-activity-bar\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.activity-rail\b/m);
    expect(shellCss).not.toMatch(/^\.activity-rail__button\b/m);
    expect(surfaceCss).toMatch(/^\.activity-rail\b/m);
    expect(surfaceCss).toMatch(/^\.activity-rail__button\b/m);
  });

  it("sizes the rail target from the emphasis control token rather than a loose literal", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/editor-activity-bar.css"),
      "utf8",
    );
    expect(surfaceCss).toMatch(
      /\.activity-rail__button\s*\{[^}]*width:\s*var\(--control-height-emphasis\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.activity-rail__button\s*\{[^}]*height:\s*var\(--control-height-emphasis\);/s,
    );
  });
});
