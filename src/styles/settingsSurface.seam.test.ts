import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Settings surface stylesheet seam", () => {
  it("owns the responsive Editor settings layout outside the window shell", async () => {
    const [entry, shellCss, settingsCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
      readFile(resolve("src/styles/settings-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/settings-surface\.css["'];/);
    expect(shellCss).not.toMatch(/^\.settings-surface\b/m);
    expect(settingsCss).toMatch(/^\.settings-surface\b/m);
    expect(settingsCss).toMatch(/^\.settings-surface__row\b/m);
    expect(settingsCss).toMatch(/@media \(max-width: 760px\)/);
    expect(settingsCss).toMatch(/@media \(forced-colors: active\)/);
  });

  it("marks the selected settings nav page with the accent selection tokens", async () => {
    const settingsCss = await readFile(
      resolve("src/styles/settings-surface.css"),
      "utf8",
    );
    const selected = settingsCss.match(
      /\.settings-surface__nav-item\[aria-selected="true"\] \{[^}]*\}/,
    );
    expect(selected).not.toBeNull();
    expect(selected?.[0]).toMatch(/var\(--highlight-selection-fill\)/);
    expect(selected?.[0]).toMatch(/var\(--highlight-selection-edge\)/);
    expect(settingsCss).not.toMatch(/--color-success|--color-warning/);
  });

  it("dims a settings nav page that has no match for the current query", async () => {
    const settingsCss = await readFile(
      resolve("src/styles/settings-surface.css"),
      "utf8",
    );
    expect(settingsCss).toMatch(
      /\.settings-surface__nav-item\[data-matches="false"\]/,
    );
  });

  it("styles the read-only keymap table without inventing a new scale", async () => {
    const settingsCss = await readFile(
      resolve("src/styles/settings-surface.css"),
      "utf8",
    );
    expect(settingsCss).toMatch(/^\.settings-surface__keymap\b/m);
    expect(settingsCss).toMatch(/var\(--font-size-xs\)/);
  });

  it("lays alternate chords for one command side by side without touching", async () => {
    const settingsCss = await readFile(
      resolve("src/styles/settings-surface.css"),
      "utf8",
    );
    const cells = [
      ...settingsCss.matchAll(/^\.settings-surface__keymap td \{[^}]*\}/gm),
    ].map((match) => match[0]);
    expect(cells.length).toBeGreaterThan(0);
    expect(
      cells.some(
        (cell) => /gap: 4px/.test(cell) && /flex-wrap: wrap/.test(cell),
      ),
    ).toBe(true);
  });
});
