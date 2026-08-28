import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Preview surface stylesheet seam", () => {
  it("loads Preview rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/preview-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/preview-surface\.css["'];/);
    expect(shellCss).not.toMatch(/^\.file-markdown-preview\b/m);
    expect(surfaceCss).toMatch(/^\.file-markdown-preview\b/m);
    expect(surfaceCss).toMatch(/^\.file-markdown-preview pre\b/m);
  });
});
