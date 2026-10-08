import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

async function composerRules() {
  const files = [
    "shared-controls.css",
    "status-surfaces.css",
    "chat-surface.css",
    "composer-surface.css",
    "composer-image.css",
  ];
  return (
    await Promise.all(
      files.map((file) => readFile(resolve("src/styles", file), "utf8")),
    )
  ).join("\n");
}

describe("Composer surface stylesheet seam", () => {
  it("keeps focused composer and related surface files below 400 lines", async () => {
    for (const file of [
      "composer-surface.css",
      "composer-image.css",
      "chat-surface.css",
      "shared-controls.css",
      "status-surfaces.css",
    ]) {
      const css = await readFile(resolve("src/styles", file), "utf8");
      expect(css.split(/\r?\n/).length, file).toBeLessThan(400);
    }
  });
  it("matches the Codex single-card footprint and round white action", async () => {
    const surfaceCss = await composerRules();
    expect(surfaceCss).toMatch(/\.composer\s*\{[^}]*max-width:\s*664px;/s);
    expect(surfaceCss).toMatch(
      /\.composer\s*\{[^}]*background:\s*var\(--composer-fill\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer\s*\{[^}]*border:\s*1px solid var\(--composer-border\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer\s*\{[^}]*border-radius:\s*var\(--radius-xl\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.send-button\s*\{[^}]*border-radius:\s*var\(--radius-circle\);[^}]*background:\s*var\(--color-status-foreground\);/s,
    );
  });

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

  it("keeps the compact controls on one line and places choices in popovers", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.composer__controls\s*\{[^}]*flex-wrap:\s*nowrap;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__options,\s*\.composer__model-menu\s*\{[^}]*position:\s*absolute;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__toolbar\s*\{[^}]*grid-template-areas:\s*"controls\s+image\s+mic\s+meter\s+send";[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto\s+auto\s+auto\s+auto;/s,
    );
  });

  it("gives harness and model menus one focused popover treatment", async () => {
    const css = await composerRules();
    expect(css).toMatch(
      /\.composer__popover\s*\{[^}]*width:\s*min\(300px,\s*calc\(100vw - 32px\)\);[^}]*gap:\s*4px;[^}]*border:\s*1px solid var\(--composer-menu-border\);[^}]*color:\s*var\(--composer-control-text\);[^}]*box-shadow:\s*var\(--composer-menu-shadow\);/s,
    );
    expect(css).toMatch(
      /\.composer__popover:focus-visible\s*\{[^}]*outline:\s*var\(--focus-ring\);/s,
    );
  });

  it("keeps natural-width composer controls available when an artifact pane narrows Chat", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.select-control\s*\{[^}]*max-width:\s*none;[^}]*grid-template-columns:\s*auto\s+minmax\(0,\s*1fr\);/s,
    );
  });

  it("centres the measured card within the chat column without hiding access", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.composer\s*\{[^}]*width:\s*min\(664px,\s*calc\(100% - 32px\)\);[^}]*margin:\s*0 auto;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer textarea\s*\{[^}]*min-height:\s*44px;[^}]*padding:\s*var\(--space-3\)\s+var\(--space-3\)\s+var\(--space-1\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__shortcut\s*\{[^}]*display:\s*none;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__error,\s*\.composer__notice\s*\{[^}]*align-items:\s*center;[^}]*align-self:\s*start;/s,
    );
    expect(surfaceCss).not.toMatch(/\.access-mode\s*\{[^}]*display:\s*none;/s);
    expect(
      await readFile(resolve("src/styles/composer-surface.css"), "utf8"),
    ).not.toMatch(/transform:\s*translate/);
  });

  it("owns an accessible token usage progressbar with tokenized and forced-color styling", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(/^\.composer__context-usage\b/m);
    expect(surfaceCss).toMatch(/^\.composer__context-usage-fill\b/m);
    expect(surfaceCss).toMatch(
      /\.composer__context-usage\s*\{[^}]*border-radius:\s*var\(--radius-pill\);[^}]*background:\s*var\(--color-border-strong\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__input-area\s*\{[^}]*display:\s*flex;[^}]*flex-direction:\s*column;/s,
    );
    const contextRule = surfaceCss.match(/^\.composer__context\s*\{([^}]*)\}/m);
    expect(contextRule).not.toBeNull();
    const contextBody = contextRule?.[1] ?? "";
    expect(contextBody).not.toMatch(/position:/);
    expect(contextBody).not.toMatch(/\b(top|right|bottom|left):/);
    expect(contextBody).toMatch(/display:\s*flex;/);
    expect(contextBody).toMatch(/justify-content:\s*flex-end;/);
    expect(contextBody).toMatch(/align-items:\s*center;/);
    expect(contextBody).toMatch(/gap:\s*var\(--space-2\);/);
    expect(contextBody).toMatch(/margin:\s*0;/);
    expect(contextBody).toMatch(/grid-area:\s*meter;/);
    expect(contextBody).toMatch(/padding:\s*0;/);
    expect(contextBody).toMatch(/font-size:\s*var\(--font-size-xs\);/);
    expect(contextBody).toMatch(/color:\s*var\(--color-text-muted\);/);
    expect(contextBody).not.toMatch(/pointer-events:\s*none;/);
    expect(surfaceCss).toMatch(
      /\.project-scope\s*\{[^}]*height:\s*var\(--control-height-compact\);[^}]*gap:\s*var\(--space-1\);[^}]*border:\s*1px solid var\(--glass-border\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.project-scope__icon,\s*\.select-control__icon\s*\{[^}]*flex:\s*0\s+0\s+auto;[^}]*color:\s*var\(--color-text-muted\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__context-usage-fill\s*\{[^}]*width:\s*var\(--composer-context-usage\);[^}]*border-radius:\s*var\(--radius-pill\);[^}]*background:\s*var\(--color-accent\);/s,
    );
    expect(
      await readFile(resolve("src/styles/status-surfaces.css"), "utf8"),
    ).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{\s*\.send-button\s*\{[^}]*border:\s*1px solid CanvasText;/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*?\.composer__context-usage\s*\{[^}]*border:[^}]*CanvasText;/s,
    );
  });

  it("keeps attached chips available without a disabled affordance row", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.composer__attachment-row\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__attachments\s*\{[^}]*flex:\s*1\s+1\s+auto;[^}]*padding:\s*0;/s,
    );
    expect(surfaceCss).toMatch(
      /\.send-button\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;[^}]*border-radius:\s*var\(--radius-circle\);[^}]*background:\s*var\(--color-status-foreground\);/s,
    );
    expect(surfaceCss).not.toContain(".composer__affordances");
    expect(surfaceCss).not.toMatch(
      /@media \(max-height:\s*360px\)[\s\S]*?\.composer__attachment-row\s*\{[^}]*display:\s*none;/s,
    );
  });

  it("uses the shared soft focus treatment and a system forced-colors edge", async () => {
    const surfaceCss = await composerRules();

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
