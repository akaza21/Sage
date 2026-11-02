import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Agent right-pane stylesheet seam", () => {
  it("keeps right-pane refinement in its owned stylesheet", async () => {
    const [entry, css, modeShell] = await Promise.all([
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
      readFile(resolve("src/styles/agent-right-pane.css"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
    ]);

    expect(entry).toContain('@import "./agent-right-pane.css";');
    expect(css).toContain(".agent-artifact-pane__follow--pending");
    expect(css).toContain(".agent-artifact-header__provenance");
    expect(css).toContain(".agent-artifact-header__view-only");
    expect(css).toContain(".agent-artifact-tab__kind");
    expect(css).toContain(".agent-preview-lifecycle--failed");
    expect(css).toContain(".harness-browser__connection--active");
    expect(css).toMatch(
      /@media \(prefers-reduced-motion: reduce\)\s*\{\s*\.agent-preview-lifecycle--starting \.agent-preview-lifecycle__icon svg\s*\{\s*animation: none;\s*\}\s*\}/,
    );
    expect(css).toMatch(
      /@media \(forced-colors: active\)\s*\{(?:(?!@media)[\s\S])*?\.agent-artifact-tab__new\s*\{[^}]*background: Highlight;[^}]*\}/,
    );
    expect(css).toMatch(
      /\.harness-browser > \.agent-preview-lifecycle\s*\{\s*grid-row: 3;/,
    );
    expect(modeShell).not.toMatch(
      /agent-artifact-(?:pane__(?:edge-copy|follow--pending|follow-new)|header__|tab__(?:kind|new))|agent-preview-lifecycle|harness-browser__connection/,
    );
    expect(css).not.toContain("writing-mode: vertical-rl");
    expect(css).not.toContain(".agent-artifact-pane__edge-copy");
    expect(css).not.toContain(".agent-artifact-pane--collapsed");
    expect(modeShell).not.toContain(".agent-artifact-pane--collapsed");
  });

  it("draws the Subagents tab person and hides child author rows (s4/f0244, f0485)", async () => {
    const [css, modeShell] = await Promise.all([
      readFile(resolve("src/styles/agent-right-pane.css"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
    ]);
    expect(css).toMatch(/\.tab__person::before\s*\{/);
    expect(css).toMatch(/\.tab__person::after\s*\{/);
    expect(modeShell).toMatch(
      /\.subagent-surface__transcript \.acp-message__meta\s*\{\s*display: none;/,
    );
  });

  it('puts the new-tab "+" right after the last tab (Codex 67)', async () => {
    const modeShell = await readFile(
      resolve("src/workbench/workbench-window-mode.css"),
      "utf8",
    );
    const strip =
      /\.agent-artifact-pane__header > \.tab-strip\s*\{([^}]*)\}/.exec(
        modeShell,
      )?.[1];
    expect(strip).toMatch(/width: auto;/);
    expect(strip).not.toMatch(/flex: 1;/);
    expect(modeShell).toMatch(
      /\.agent-artifact-pane__header > \.agent-artifact-pane__new-tab\s*\{\s*order: 1;\s*margin-right: auto;\s*\}/,
    );
    expect(modeShell).toMatch(
      /\.agent-artifact-pane__follow,\s*\.agent-artifact-pane__collapse\s*\{\s*order: 2;\s*\}/,
    );
  });
});
