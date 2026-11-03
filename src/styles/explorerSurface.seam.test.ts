import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Explorer stylesheet seam", () => {
  it("loads Explorer rules from the application entry, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss, sharedControlsCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/features/files/explorer-surface.css"), "utf8"),
      readFile(resolve("src/styles/shared-controls.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/features\/files\/explorer-surface\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.explorer-action-menu\b/m);
    expect(shellCss).not.toMatch(/^\.root-tree__heading\b/m);
    expect(surfaceCss).toMatch(/^\.explorer-action-menu\b/m);
    expect(surfaceCss).not.toMatch(/@media \(forced-colors: active\)/);
    expect(sharedControlsCss).toMatch(/@media \(forced-colors: active\)/);
    expect(surfaceCss).toMatch(/^\.root-tree__heading\b/m);
  });
});
