import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const themeSource = readFileSync(
  resolve(process.cwd(), "src/styles/global.css"),
  "utf8",
);
const rootBlock = themeSource.match(/:root\s*\{(?<declarations>[^}]*)\}/)
  ?.groups?.declarations;
const forcedColorsStart = themeSource.indexOf("@media (forced-colors: active)");
const ordinaryThemeSource = themeSource.slice(0, forcedColorsStart);
const forcedColorsSource = themeSource.slice(forcedColorsStart);

describe("Devin-dark theme tokens", () => {
  it("uses a neutral near-black canvas and reserves blue for meaning", () => {
    expect(rootBlock).toBeDefined();
    expect(rootBlock).toMatch(/--color-canvas:\s*#141414;/);
    expect(rootBlock).toMatch(/--color-accent:\s*#4c9df3;/);
    expect(rootBlock).toMatch(/--color-accent-hover:\s*#75b4f7;/);
    expect(rootBlock).toMatch(/--bg:\s*var\(--color-canvas\);/);
    expect(rootBlock).toMatch(/--surface:\s*var\(--color-panel\);/);
    expect(rootBlock).toMatch(/--surface-raised:\s*var\(--color-surface\);/);
    expect(rootBlock).toMatch(/--text:\s*var\(--color-text-primary\);/);
    expect(rootBlock).toMatch(/--accent:\s*var\(--color-accent\);/);
    expect(rootBlock).toMatch(
      /--highlight-selection-fill:\s*color-mix\(\s*in srgb,\s*var\(--color-accent\) 12%,\s*var\(--color-surface\)\s*\);/,
    );
    expect(rootBlock).toMatch(
      /--highlight-active-fill:\s*color-mix\(\s*in srgb,\s*var\(--color-accent\) 7%,\s*var\(--color-surface-hover\)\s*\);/,
    );
    expect(rootBlock).not.toMatch(
      /#0e1116|#141820|#1a1f28|#222833|#2a3140|#2c3340|#3a4252|#e8edf5|#b4bdc9|#8b95a5|#687282|#1a2d4d|#9ad19b|#101210|#b5e4b4|#253c2a/,
    );
    expect(themeSource).not.toMatch(/#9ad19b|#101210/);
  });

  it("keeps the default authority safe for Agent portals and forced colors", () => {
    expect(forcedColorsStart).toBeGreaterThan(-1);
    expect(themeSource).not.toMatch(
      /\.workbench--agent[^{]*\{[^}]*--(?:doc|diff)-[\w-]+\s*:/s,
    );
    expect(ordinaryThemeSource).not.toMatch(/\.workbench--agent[^{]*\{/);
    expect(themeSource).not.toContain("@media (forced-colors: none)");
    expect(forcedColorsSource).toMatch(
      /:root\s*\{[\s\S]*--color-canvas:\s*Canvas;[\s\S]*--color-surface-active:\s*Highlight;[\s\S]*--color-accent:\s*LinkText;[\s\S]*--highlight-focus-edge:\s*Highlight;/,
    );
  });

  it("defines a compact chrome ramp apart from Chat reading text", () => {
    expect(rootBlock).toMatch(/--font-size-xs:\s*11px;/);
    expect(rootBlock).toMatch(/--font-size-sm:\s*12px;/);
    expect(rootBlock).toMatch(/--font-size-md:\s*14px;/);
    expect(rootBlock).toMatch(/--font-size-xl:\s*20px;/);
    expect(rootBlock).toMatch(/--type-size-chrome:\s*var\(--font-size-sm\);/);
    expect(rootBlock).toMatch(/--type-size-chat:\s*var\(--font-size-md\);/);
  });
});
