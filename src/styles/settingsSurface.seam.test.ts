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
});
