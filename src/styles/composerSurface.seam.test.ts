import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Composer surface stylesheet seam", () => {
  it("loads composer rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/composer-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/composer-surface\.css["'];/);
    expect(shellCss).not.toMatch(/^\.composer\b/m);
    expect(shellCss).not.toMatch(/^\.select-control\b/m);
    expect(surfaceCss).toMatch(/^\.composer\b/m);
    expect(surfaceCss).toMatch(/^\.select-control\b/m);
  });

  it("wraps composer controls instead of crowding the send action", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/composer-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.composer__controls\s*\{[^}]*flex-wrap:\s*wrap;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__controls\s*>\s*\.select-control\s*\{[^}]*flex:\s*0\s+1\s+auto;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__toolbar\s*\{[^}]*grid-template-areas:\s*"controls\s+send";[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto;/s,
    );
  });

  it("keeps the conversation composer compact without moving controls on hover", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/composer-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.composer textarea\s*\{[^}]*min-height:\s*76px;[^}]*padding:\s*var\(--space-3\)\s+var\(--space-3\)\s+var\(--space-6\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__shortcut\s*\{[^}]*display:\s*none;/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(max-width:\s*820px\)[\s\S]*\.access-mode\s*\{[^}]*display:\s*none;/s,
    );
    expect(surfaceCss).not.toMatch(/transform:\s*translate/);
  });

  it("owns an accessible token usage progressbar with tokenized and forced-color styling", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/composer-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(/^\.composer__context-usage\b/m);
    expect(surfaceCss).toMatch(/^\.composer__context-usage-fill\b/m);
    expect(surfaceCss).toMatch(
      /\.composer__context-usage\s*\{[^}]*border-radius:\s*var\(--radius-pill\);[^}]*background:\s*var\(--color-border-strong\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__context\s*\{[^}]*position:\s*absolute;[^}]*right:\s*var\(--space-3\);[^}]*bottom:\s*var\(--space-2\);[^}]*pointer-events:\s*none;/s,
    );
    expect(surfaceCss).toMatch(
      /\.project-scope\s*\{[^}]*height:\s*var\(--control-height-compact\);[^}]*gap:\s*var\(--space-1\);[^}]*border:\s*1px solid var\(--color-border-subtle\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.project-scope__icon,\s*\.select-control__icon\s*\{[^}]*flex:\s*0\s+0\s+auto;[^}]*color:\s*var\(--color-text-muted\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__context-usage-fill\s*\{[^}]*width:\s*var\(--composer-context-usage\);[^}]*border-radius:\s*var\(--radius-pill\);[^}]*background:\s*var\(--color-accent\);/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*?\.project-scope__icon,\s*\.select-control__icon,\s*[\s\S]*?\.send-button\s*\{[^}]*border:\s*1px solid CanvasText;/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*?\.composer__context-usage\s*\{[^}]*border:[^}]*CanvasText;/s,
    );
  });

  it("places disabled affordances and removable attachment chips on one row", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/composer-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.composer__attachment-row\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__attachments\s*\{[^}]*flex:\s*1\s+1\s+auto;[^}]*padding:\s*0;/s,
    );
    expect(surfaceCss).toMatch(
      /\.send-button\s*\{[^}]*width:\s*var\(--control-height-emphasis\);[^}]*height:\s*var\(--control-height-emphasis\);[^}]*background:\s*var\(--color-accent\);/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(max-height:\s*360px\)[\s\S]*?\.composer__affordances\s*\{[^}]*display:\s*none;/s,
    );
    expect(surfaceCss).not.toMatch(
      /@media \(max-height:\s*360px\)[\s\S]*?\.composer__attachment-row\s*\{[^}]*display:\s*none;/s,
    );
  });

  it("uses the shared soft focus treatment and a system forced-colors edge", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/composer-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.composer:focus-within\s*\{[^}]*border-color:\s*var\(--highlight-focus-edge\);[^}]*box-shadow:\s*var\(--highlight-focus-halo-shadow\),\s*var\(--elevation-popover\);/s,
    );
    expect(surfaceCss).not.toMatch(
      /\.composer:focus-within\s*\{[^}]*0\s+0\s+0\s+var\(--focus-ring-offset\)/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*?\.composer:focus-within\s*\{[^}]*border-color:\s*Highlight;[^}]*outline:\s*2px solid Highlight;[^}]*outline-offset:\s*-2px;[^}]*box-shadow:\s*none;/s,
    );
  });
});
