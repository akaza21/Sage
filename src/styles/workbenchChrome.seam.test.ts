import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Workbench chrome stylesheet seam", () => {
  it("loads title and tab chrome from a focused stylesheet", async () => {
    const [entry, shellCss, chromeCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/workbench-chrome.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/workbench-chrome\.css["'];/);
    expect(shellCss).not.toMatch(/^\.title-bar\b/m);
    expect(shellCss).not.toMatch(/^\.tab-strip\b/m);
    expect(chromeCss).toMatch(/^\.title-bar\b/m);
    expect(chromeCss).toMatch(/^\.tab-strip\b/m);
  });

  it("keeps title and Editor tab controls on bounded targets without narrow overlap", async () => {
    const chromeCss = await readFile(
      resolve("src/styles/workbench-chrome.css"),
      "utf8",
    );

    expect(chromeCss).toMatch(/\.title-bar__end\s*\{[^}]*min-width:\s*0;/s);
    expect(chromeCss).toMatch(
      /\.tab-strip__new\s*\{[^}]*width:\s*var\(--control-height-compact\);[^}]*height:\s*100%;/s,
    );
    expect(chromeCss).toMatch(
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.title-bar\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto;/,
    );
    expect(chromeCss).toMatch(
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.title-bar__start\s*\{[^}]*flex-wrap:\s*wrap;/,
    );
  });
});
