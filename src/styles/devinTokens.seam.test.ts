import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("Devin / VS Code dark tokens", () => {
  it("keeps the default canvas neutral and meaningful states blue", () => {
    const css = [
      readFileSync(resolve("src/styles/global.css"), "utf8"),
      readFileSync(resolve("src/features/search/workspace-search.css"), "utf8"),
    ].join("\n");
    expect(css).not.toMatch(/#0f110f|#2b4932|#31523a|#263f2c|#465248|#526354/);
    expect(css).toContain("--color-accent: #4c9df3");
    expect(css).toContain("--color-canvas: #141414");
    expect(css).toContain("--color-surface-active: #2a4b6d");
    expect(css).toContain("--color-selection: #264f78");
    expect(css).toContain("--editor: var(--color-canvas)");
  });
});
