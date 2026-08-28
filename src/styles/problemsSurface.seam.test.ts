import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Problems surface stylesheet seam", () => {
  it("loads Problems rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/problems-outline-surfaces.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/problems-outline-surfaces\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.problems-surface\b/m);
    expect(surfaceCss).toMatch(/^\.problems-surface\b/m);
    expect(surfaceCss).toMatch(/^\.problem-card\b/m);
  });
});
