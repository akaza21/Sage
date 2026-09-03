import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("shared controls", () => {
  it("loads one shared form and popover language before feature styles", async () => {
    const [entry, shared, global, file] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/shared-controls.css"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/file-editor-surface.css"), "utf8"),
    ]);

    expect(
      entry.indexOf('import "./styles/shared-controls.css";'),
    ).toBeGreaterThan(entry.indexOf('import "./styles/global.css";'));
    expect(shared.match(/^\.menu-popover\s*\{/gm)).toHaveLength(1);
    expect(shared.match(/^\.menu-item\s*\{/gm)).toHaveLength(1);
    expect(shared).toMatch(/^\.input,/m);
    expect(shared).toMatch(/^\.button,/m);
    expect(shared).toContain(".select");
    expect(shared).toContain("@media (forced-colors: active)");
    expect(shared).toContain("@media (prefers-reduced-motion: reduce)");
    expect(global).not.toMatch(/^\.button\s*\{/m);
    expect(file).not.toMatch(/^\.button--(compact|primary|danger)\s*\{/m);
  });
});
