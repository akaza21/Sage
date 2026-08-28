import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("Devin / VS Code dark tokens", () => {
  it("keeps the editor canvas on graphite-blue, not leftover sage-green", () => {
    const css = [
      readFileSync(resolve("src/styles/global.css"), "utf8"),
      readFileSync(resolve("src/features/search/workspace-search.css"), "utf8"),
    ].join("\n");
    expect(css).not.toMatch(/#0f110f|#2b4932|#31523a|#263f2c|#465248|#526354/);
    expect(css).toContain("--accent: #4c8dff");
    expect(css).toContain("--editor: #0e1116");
    expect(css).toContain("--selection: #264f78");
  });
});
