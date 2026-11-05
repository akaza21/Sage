import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Status surfaces stylesheet seam", () => {
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

  it("anchors the placeholder heading to the large font token rather than a literal", async () => {
    const [surfaceCss, editorCss] = await Promise.all([
      readFile(resolve("src/styles/status-surfaces.css"), "utf8"),
      readFile(resolve("src/styles/editor-visual-reference.css"), "utf8"),
    ]);

    expect(surfaceCss).toMatch(
      /\.placeholder-surface h1 \{[^}]*font-size: var\(--font-size-lg\);/s,
    );
    expect(surfaceCss).not.toMatch(/font-size:\s*19px/);
    expect(editorCss).not.toMatch(
      /\.workbench--editor \.placeholder-surface h1 \{[^}]*font-size:/s,
    );
  });
});
