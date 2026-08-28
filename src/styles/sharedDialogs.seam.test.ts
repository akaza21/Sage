import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Shared dialogs stylesheet seam", () => {
  it("loads shared dialog rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/shared-dialogs.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/shared-dialogs\.css["'];/);
    expect(shellCss).not.toMatch(/^\.dialog-backdrop\b/m);
    expect(shellCss).not.toMatch(/^\.dirty-dialog\b/m);
    expect(shellCss).not.toMatch(/^\.chat-search-dialog\b/m);
    expect(surfaceCss).toMatch(/^\.dialog-backdrop\b/m);
    expect(surfaceCss).toMatch(/^\.dirty-dialog\b/m);
    expect(surfaceCss).toMatch(/^\.new-file-dialog\b/m);
    expect(surfaceCss).toMatch(/^\.handoff-dialog\b/m);
    expect(surfaceCss).toMatch(/^\.chat-search-dialog\b/m);
  });
});
