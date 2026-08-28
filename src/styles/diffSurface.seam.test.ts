import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Diff surface stylesheet seam", () => {
  it("loads Diff rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/diff-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/diff-surface\.css["'];/);
    expect(shellCss).not.toMatch(/^\.diff-surface\b/m);
    expect(shellCss).not.toMatch(/^\.diff-line\b/m);
    expect(shellCss).not.toMatch(/\.diff-limit\b/);
    expect(surfaceCss).toMatch(/^\.diff-surface\b/m);
    expect(surfaceCss).toMatch(/^\.diff-line\b/m);
  });
});
