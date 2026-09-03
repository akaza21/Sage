import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Right-region stylesheet seam", () => {
  it("loads right-region layout primitives from workbench chrome, not the shell stylesheet", async () => {
    const [entry, shellCss, chromeCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/workbench-chrome.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/workbench-chrome\.css["'];/);
    expect(shellCss).not.toMatch(/^\.right-region\b/m);
    expect(shellCss).not.toMatch(/^\.right-region--collapsed\b/m);
    expect(shellCss).not.toMatch(/^\.right-region--leading\b/m);
    expect(chromeCss).toMatch(/^\.right-region\b/m);
    expect(chromeCss).toMatch(/^\.right-region\s*\{[\s\S]*?min-height:\s*0;/m);
    expect(chromeCss).toMatch(/^\.right-region--collapsed\b/m);
    expect(chromeCss).toMatch(/^\.right-region--leading\b/m);
  });

  it("keeps compact-width right-region overlay rules on the chrome surface", async () => {
    const [shellCss, chromeCss] = await Promise.all([
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/workbench-chrome.css"), "utf8"),
    ]);

    expect(shellCss).not.toMatch(/\.right-region\s+\.sidebar\b/);
    expect(shellCss).not.toMatch(/\.right-region--leading\s+\.sidebar\b/);
    expect(chromeCss).toMatch(
      /@media \(max-width: 760px\)[\s\S]*\.right-region\s+\.sidebar\b/,
    );
    expect(chromeCss).toMatch(
      /@media \(max-width: 760px\)[\s\S]*\.right-region--leading\s+\.sidebar\b/,
    );
  });
});
