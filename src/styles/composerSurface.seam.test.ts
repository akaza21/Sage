import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Composer surface stylesheet seam", () => {
  it("loads composer rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/composer-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/composer-surface\.css["'];/);
    expect(shellCss).not.toMatch(/^\.composer\b/m);
    expect(shellCss).not.toMatch(/^\.select-control\b/m);
    expect(surfaceCss).toMatch(/^\.composer\b/m);
    expect(surfaceCss).toMatch(/^\.select-control\b/m);
  });

  it("wraps composer controls instead of crowding the send action", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/composer-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.composer__controls\s*\{[^}]*flex-wrap:\s*wrap;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__toolbar\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto;/s,
    );
  });
});
