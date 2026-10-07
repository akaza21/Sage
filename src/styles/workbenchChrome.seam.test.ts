import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Workbench chrome stylesheet seam", () => {
  it("uses the measured Agent centre and right-panel chrome", async () => {
    const css = await readFile(
      resolve("src/styles/workbench-chrome.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.workbench--agent\s*\{[^}]*--chrome-row-height:\s*48px;[^}]*--title-bar-fill:\s*#111111;/s,
    );
    expect(css).toMatch(
      /\.workbench--agent \.agent-chat-center,\s*\.workbench--agent \.agent-artifact-pane\s*\{[^}]*background:\s*var\(--agent-shell-center\);/s,
    );
    expect(css).toMatch(
      /\.workbench--agent \.agent-artifact-pane__header\s*\{[^}]*height:\s*44px;[^}]*border-bottom:\s*1px solid var\(--agent-shell-divider\);/s,
    );
    expect(css).toMatch(
      /\.workbench--agent \.agent-artifact-pane \.tab--active\s*\{[^}]*background:\s*var\(--color-plane-raised\);[^}]*border:\s*1px solid var\(--agent-shell-tab-border\);/s,
    );
  });
  it("keeps the selected chat as the sole filled sidebar row and section labels left aligned", async () => {
    const css = await readFile(
      resolve("src/styles/agent-rail-surface.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.workbench--agent \.project-row--active\s*\{[^}]*background:\s*transparent;/s,
    );
    expect(css).toMatch(
      /\.agent-rail-section__disclosure > svg\s*\{[^}]*position:\s*absolute;[^}]*inset-inline-end:\s*var\(--space-1\);[^}]*opacity:\s*0;/s,
    );
  });
  it("matches every artifact grid header row to the rendered header height", async () => {
    const [chromeCss, modeCss] = await Promise.all([
      readFile(resolve("src/styles/workbench-chrome.css"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
    ]);
    const headerHeight =
      /\.workbench--agent \.agent-artifact-pane__header\s*\{[^}]*height:\s*(\d+px);/s.exec(
        chromeCss,
      )?.[1];

    expect(headerHeight).toBe("44px");
    for (const selector of [
      /\.agent-artifact-pane\s*\{[^}]*grid-template-rows:\s*(\d+px)\s+minmax\(0, 1fr\);/s,
      /\.agent-artifact-pane--empty\s*\{[^}]*grid-template-rows:\s*(\d+px)\s+minmax\(0, 1fr\);/s,
      /\.agent-artifact-pane--with-header\s*\{[^}]*grid-template-rows:\s*(\d+px)\s+auto\s+minmax\(0, 1fr\);/s,
    ]) {
      expect(selector.exec(modeCss)?.[1]).toBe(headerHeight);
    }
  });
  it("loads title and tab chrome from a focused stylesheet", async () => {
    const [entry, shellCss, chromeCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/workbench-chrome.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/workbench-chrome\.css["'];/);
    expect(shellCss).not.toMatch(/^\.title-bar\b/m);
    expect(shellCss).not.toMatch(/^\.tab-strip\b/m);
    expect(chromeCss).toMatch(/^\.title-bar\b/m);
    expect(chromeCss).toMatch(/^\.tab-strip\b/m);
  });

  it("sizes the chat search pill to the owner reference with a trailing chord", async () => {
    const chromeCss = await readFile(
      resolve("src/styles/workbench-chrome.css"),
      "utf8",
    );

    expect(chromeCss).toMatch(
      /\.title-bar__chat-search--pill\s*\{[^}]*width:\s*var\(--title-bar-search-width\);[^}]*height:\s*var\(--title-bar-search-height\);/s,
    );
    expect(chromeCss).toMatch(
      /\.title-bar__chat-search--pill\s*\{[^}]*font-size:\s*var\(--font-size-sm\);/s,
    );
    expect(chromeCss).toMatch(
      /\.title-bar__chord\s*\{[^}]*margin-left:\s*auto;/s,
    );
  });

  it("keeps title and Editor tab controls on bounded targets without narrow overlap", async () => {
    const chromeCss = await readFile(
      resolve("src/styles/workbench-chrome.css"),
      "utf8",
    );

    expect(chromeCss).toMatch(/\.title-bar__end\s*\{[^}]*min-width:\s*0;/s);
    expect(chromeCss).toMatch(
      /\.tab-strip__new\s*\{[^}]*width:\s*var\(--control-height-compact\);[^}]*height:\s*100%;/s,
    );
    expect(chromeCss).toMatch(
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.title-bar\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto;/,
    );
    expect(chromeCss).toMatch(
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.title-bar__start\s*\{[^}]*flex-wrap:\s*wrap;/,
    );
  });

  it("marks the active window mode segment with the accent selection tokens", async () => {
    const windowModeCss = await readFile(
      resolve("src/workbench/workbench-window-mode.css"),
      "utf8",
    );

    const active = /\.title-bar__mode--active\s*\{([^}]*)\}/s.exec(
      windowModeCss,
    );

    expect(active).not.toBeNull();
    expect(active?.[1]).toMatch(
      /background:\s*var\(--highlight-selection-fill\);/,
    );
    expect(active?.[1]).toMatch(
      /box-shadow:\s*inset 0 0 0 1px var\(--highlight-selection-edge\);/,
    );
    expect(active?.[1]).not.toMatch(/--color-surface-active/);
  });
});
