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

  it("reuses the shared menu classes without nesting a select", async () => {
    const [component, surfaceCss] = await Promise.all([
      readFile(resolve("src/workbench/AgentRailFilterMenu.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
    ]);

    expect(surfaceCss).toMatch(
      /\.agent-rail-status__header\s*\{[^}]*font-size:\s*var\(--font-size-xs\);/s,
    );
    expect(component).toContain('className="menu-item"');
    expect(component).toContain('className="menu-group-label"');
    expect(component).not.toContain("<select");
    expect(component).toContain('role="menuitemradio"');
    expect(surfaceCss).not.toMatch(
      /\.agent-rail-filter__menu\s*\{[^}]*position:\s*absolute/s,
    );
  });

  it("keeps the rail filter trigger and menu actions on consistent targets", async () => {
    const [component, surfaceCss] = await Promise.all([
      readFile(resolve("src/workbench/AgentRailFilterMenu.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
    ]);

    expect(surfaceCss).toMatch(
      /\.agent-rail-filter__trigger\s*\{[^}]*width:\s*var\(--control-height-compact\);[^}]*height:\s*var\(--control-height-compact\);/s,
    );
    expect(component).toContain("<MenuPopover");
    expect(component).toContain("trapFocus");
    expect(component).toContain('aria-haspopup="menu"');
  });
});
