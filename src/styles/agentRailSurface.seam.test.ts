import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Agent rail stylesheet seam", () => {
  it("loads Agent rail rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/agent-rail-surface\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.agent-rail-status\b/m);
    expect(shellCss).not.toMatch(/^\.agent-rail-filter__menu\b/m);
    expect(surfaceCss).toMatch(/^\.agent-rail-status\b/m);
    expect(surfaceCss).toMatch(/^\.agent-rail-filter__menu\b/m);
  });

  it("uses the compact chrome ramp for rail labels", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/agent-rail-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.agent-rail-status__header\s*\{[^}]*font-size:\s*var\(--font-size-xs\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.agent-rail-filter__menu button,[\s\S]*?\.agent-rail-filter__select\s*\{[^}]*font-size:\s*var\(--font-size-sm\);/s,
    );
  });

  it("keeps the rail filter trigger and menu actions on consistent targets", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/agent-rail-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.agent-rail-filter__trigger\s*\{[^}]*width:\s*var\(--control-height-compact\);[^}]*height:\s*var\(--control-height-compact\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.agent-rail-filter__menu button,[\s\S]*?\.agent-rail-filter__select\s*\{[^}]*min-height:\s*var\(--control-height-compact\);/s,
    );
  });
});
