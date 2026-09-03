import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Repository sidebar controls stylesheet seam", () => {
  it("loads repository sidebar controls from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/repository-sidebar-controls.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/repository-sidebar-controls\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.source-control-list\b/m);
    expect(shellCss).not.toMatch(/^\.repository-commit\b/m);
    expect(surfaceCss).toMatch(/^\.sidebar__heading\b/m);
    expect(surfaceCss).toMatch(/^\.source-control-list\b/m);
    expect(surfaceCss).toMatch(/^\.source-root__summary\b/m);
    expect(surfaceCss).toMatch(/^\.repository-commit\b/m);
  });
});
