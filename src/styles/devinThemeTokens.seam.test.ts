import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

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
const MAX_SOURCE_ENTRIES = 20_000;
const MAX_SOURCE_STYLESHEETS = 500;

function readSourceStylesheets() {
  const sourceRoot = resolve(process.cwd(), "src");
  const pendingDirectories = [sourceRoot];
  const stylesheets: Array<{ path: string; source: string }> = [];
  let visitedEntries = 0;

  while (pendingDirectories.length > 0) {
    const directory = pendingDirectories.pop();
    if (directory == null) break;
    const entries = readdirSync(directory, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      visitedEntries += 1;
      if (visitedEntries > MAX_SOURCE_ENTRIES) {
        throw new Error(
          `Source stylesheet inventory exceeded ${MAX_SOURCE_ENTRIES} entries`,
        );
      }
      const absolutePath = join(directory, entry.name);
      if (entry.isDirectory()) {
        pendingDirectories.push(absolutePath);
      } else if (entry.isFile() && entry.name.endsWith(".css")) {
        if (stylesheets.length >= MAX_SOURCE_STYLESHEETS) {
          throw new Error(
            `Source stylesheet inventory exceeded ${MAX_SOURCE_STYLESHEETS} CSS files`,
          );
        }
        stylesheets.push({
          path: relative(sourceRoot, absolutePath),
          source: readFileSync(absolutePath, "utf8"),
        });
      }
    }
  }

  return stylesheets.sort((left, right) => left.path.localeCompare(right.path));
}

describe("Devin-dark theme tokens", () => {
  it("uses a neutral near-black canvas and reserves blue for meaning", () => {
    expect(rootBlock).toBeDefined();
    expect(rootBlock).toMatch(/--color-canvas:\s*#141414;/);
    expect(rootBlock).toMatch(/--color-accent:\s*#4c9df3;/);
    expect(rootBlock).toMatch(/--color-accent-hover:\s*#75b4f7;/);
    expect(rootBlock).toMatch(/--color-plane-raised:\s*#2a2a2a;/);
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

  it("reserves active and selected surface roles for intentional states", () => {
    const activeSurfaceConsumers = readSourceStylesheets().flatMap(
      (stylesheet) =>
        [
          ...stylesheet.source.matchAll(
            /(?<selector>[^{}]+)\{(?<body>[^{}]*)\}/g,
          ),
        ]
          .filter((match) => {
            if (match.groups?.selector.trim().endsWith(":root")) return false;
            const body = match.groups?.body ?? "";
            return [
              "var(--color-surface-active)",
              "var(--color-surface-selected)",
            ].some((token) => body.includes(token));
          })
          .map((match) => ({
            path: stylesheet.path.replaceAll("\\", "/"),
            selector: match.groups?.selector.replace(/\s+/g, " ").trim(),
          })),
    );

    expect(activeSurfaceConsumers).toEqual([
      {
        path: "features/chat/acp/acp-chat.css",
        selector: ".acp-message--user",
      },
      {
        path: "features/files/explorer-surface.css",
        selector: ".tree__row--selected",
      },
      {
        path: "features/search/workspace-search.css",
        selector:
          '.workspace-search__query-actions button[aria-pressed="true"]',
      },
      {
        path: "styles/chat-navigation-surface.css",
        selector: ".project-chat-row--active",
      },
      {
        path: "styles/chat-surface.css",
        selector: ".message--user",
      },
      {
        path: "styles/editor-activity-bar.css",
        selector: ".activity-rail__button--active",
      },
      {
        path: "styles/editor-interactions.css",
        selector:
          ".workbench--editor :where( .title-bar__search, .title-bar__mode, .activity-rail__button, .sidebar__heading button, .file-header__preview, .status-bar__button, .tab__activate, .tab__close, .tree__row, .root-tree__heading, .source-change, .source-change__action, .source-change__resolve, .source-change__confirmation-button, .source-branch__switch, .source-branch__delete, .source-branch-create button, .source-remote__action, .repository-commit__footer button, .workspace-search__details-toggle, .workspace-search__query-actions button, .workspace-search__input button, .workspace-search__result, .problem-card, .outline-card, .editor-find__button, .editor-welcome__actions button ):active:not(:disabled)",
      },
      {
        path: "styles/editor-interactions.css",
        selector:
          '.workbench--editor :where( .file-header__preview, .status-bar__button, .editor-find__button, .workspace-search__query-actions button )[aria-pressed="true"]',
      },
      {
        path: "styles/file-editor-surface.css",
        selector: '.file-header__preview[aria-pressed="true"]',
      },
      {
        path: "styles/source-control-changes.css",
        selector: ".source-change--selected",
      },
      {
        path: "workbench/workbench-window-mode.css",
        selector: ".title-bar__mode--active",
      },
      {
        path: "workbench/workbench-window-mode.css",
        selector: '.agent-artifact-pane__follow[aria-pressed="true"]',
      },
    ]);
  });

  it("keeps the default authority safe for Agent portals and forced colors", () => {
    expect(forcedColorsStart).toBeGreaterThan(-1);
    expect(themeSource).not.toMatch(
      /\.workbench--agent[^{]*\{[^}]*--(?:doc|diff)-[\w-]+\s*:/s,
    );
    expect(ordinaryThemeSource).not.toMatch(/\.workbench--agent[^{]*\{/);
    expect(themeSource).not.toContain("@media (forced-colors: none)");
    expect(forcedColorsSource).toMatch(
      /:root\s*\{[\s\S]*--color-canvas:\s*Canvas;[\s\S]*--color-plane-raised:\s*Canvas;[\s\S]*--color-surface-active:\s*Highlight;[\s\S]*--color-accent:\s*LinkText;[\s\S]*--highlight-focus-edge:\s*Highlight;/,
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
