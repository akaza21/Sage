import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Editor interaction-state seam", () => {
  it("loads one Editor-only interaction layer after the visual projection", async () => {
    const [entry, css] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/editor-interactions.css"), "utf8"),
    ]);

    expect(
      entry.indexOf('import "./styles/editor-interactions.css"'),
    ).toBeGreaterThan(
      entry.indexOf('import "./styles/editor-visual-reference.css"'),
    );
    expect(css).not.toMatch(/^:root\b/m);
    expect(css).toContain(":active:not(:disabled)");
    expect(css).toContain('[aria-pressed="true"]');
    expect(css).toContain("@media (prefers-reduced-motion: reduce)");
    expect(css).toContain("@media (forced-colors: active)");
    expect(css).not.toMatch(/transform:\s*(translate|scale)/);
  });

  it("covers the visible Editor control families with a stable press state", async () => {
    const css = await readFile(
      resolve("src/styles/editor-interactions.css"),
      "utf8",
    );

    for (const selector of [
      ".title-bar__search",
      ".activity-rail__button",
      ".file-header__preview",
      ".status-bar__button",
      ".tab__activate",
      ".source-change__action",
      ".source-branch__switch",
      ".source-branch__delete",
      ".source-branch-create button",
      ".source-remote__action",
      ".repository-commit__footer button",
      ".workspace-search__controls button",
      ".problem-card",
      ".outline-card",
      ".editor-welcome__actions button",
    ]) {
      expect(css).toContain(selector);
    }
  });
});
