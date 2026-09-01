import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Source Control changes stylesheet seam", () => {
  it("loads Git change rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/source-control-changes.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/source-control-changes\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.source-change-row\b/m);
    expect(shellCss).not.toMatch(/^\.source-change__discard-confirmation\b/m);
    expect(surfaceCss).toMatch(/^\.source-change-row\b/m);
    expect(surfaceCss).toMatch(/^\.source-change__discard-confirmation\b/m);
    expect(surfaceCss).toMatch(
      /\.source-change--selected\s*\{[^}]*background:\s*var\(--color-surface-selected\)[^}]*box-shadow:[^}]*font-weight:/s,
    );
    expect(surfaceCss).toMatch(
      /\.source-change:hover\s*\{[^}]*background:\s*var\(--color-surface-hover\)/s,
    );
    expect(surfaceCss).not.toMatch(
      /\.source-change:hover\s*,\s*\.source-change--selected/,
    );
    expect(surfaceCss).toContain('.source-change[aria-current="true"]');
  });
});
