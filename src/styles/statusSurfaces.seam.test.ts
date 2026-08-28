import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Status surfaces stylesheet seam", () => {
  it("loads shared status and placeholder rules from a focused stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/status-surfaces.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/status-surfaces\.css["'];/);
    expect(shellCss).not.toMatch(/^\.resource-state\b/m);
    expect(shellCss).not.toMatch(/^\.placeholder-surface\b/m);
    expect(shellCss).not.toMatch(/^\.status-bar\b/m);
    expect(surfaceCss).toMatch(/^\.resource-state\b/m);
    expect(surfaceCss).toMatch(/^\.placeholder-surface\b/m);
    expect(surfaceCss).toMatch(/^\.status-bar\b/m);
  });

  it("keeps the editor appearance controls as a bounded status-bar group", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/status-surfaces.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.status-bar__editor-settings\s*\{[^}]*flex:\s*0 0 auto;/s,
    );
    expect(surfaceCss).toMatch(
      /\.status-bar__editor-settings\s*\{[^}]*border-inline-start:/s,
    );
    expect(surfaceCss).toMatch(/\.status-bar\s*\{[^}]*overflow-x:\s*auto;/s);
  });
});
