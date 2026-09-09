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

  it("keeps informational editor status compact without appearance controls", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/status-surfaces.css"),
      "utf8",
    );

    expect(surfaceCss).not.toContain(".status-bar__editor-settings");
    expect(surfaceCss).toMatch(
      /\.status-bar__group span\s*\{[^}]*white-space:\s*nowrap;/s,
    );
    expect(surfaceCss).toMatch(/\.status-bar\s*\{[^}]*overflow-x:\s*auto;/s);
  });

  it("keeps Source Control notices compact without changing generic notices", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/status-surfaces.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.source-control-list\s*>\s*\.resource-state\s*\{[^}]*height:\s*auto;[^}]*gap:\s*var\(--space-1\);[^}]*padding:\s*var\(--space-2\) 0;/s,
    );
    expect(surfaceCss).toMatch(
      /\.source-control-list\s*>\s*\.resource-state\s*>\s*\.resource-state__icon\s*\{[^}]*width:\s*var\(--space-4\);[^}]*height:\s*var\(--space-4\);[^}]*border:\s*0;[^}]*background:\s*transparent;/s,
    );
    expect(surfaceCss).toMatch(
      /^\.resource-state__icon,\n\.placeholder-surface__icon/m,
    );
  });
});
