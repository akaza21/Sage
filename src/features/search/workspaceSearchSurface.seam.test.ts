import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Workspace Search stylesheet seam", () => {
  it("loads Search rules from the application entry, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/features/search/workspace-search.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/features\/search\/workspace-search\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.workspace-search\b/m);
    expect(surfaceCss).toMatch(/^\.workspace-search\b/m);
    expect(surfaceCss).toMatch(/^\.workspace-search__result\b/m);
  });
});
