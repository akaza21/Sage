import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const preQ05DuplicateSelectors = [
  ".acp-chat-projection",
  ".acp-code-block",
  ".acp-message--user",
  ".acp-permission header",
  ".acp-tool-command",
  ".acp-turn + .acp-turn",
  ".acp-turn__header",
  ".dirty-dialog--editor",
  ".menu-popover--editor",
  ".message-list",
  ".new-file-dialog--editor",
  ".project-chat",
  ".session-state",
  ".surface-header",
  ".title-bar__start",
  ".workbench--editor",
  ".workbench--editor .activity-rail__button",
  ".workbench--editor .activity-rail__button--active",
  ".workbench--editor .right-region--leading",
  ".workbench--editor .right-region--leading .activity-rail__button--active::after",
  ".workbench--editor .sidebar",
  ".workbench--editor .sidebar:focus-visible",
  ".workbench--editor .source-control-selector select:focus-visible",
  ".workbench--editor .status-bar__button",
  ".workbench--editor .status-bar__button:focus-visible",
  ".workbench--editor .tab--active",
  ".workbench--editor .tree__row--selected",
  ".workbench--editor .workbench-sidebar-splitter--active::after",
  ".workbench--editor .workbench-sidebar-splitter::after",
  ".workbench--editor .workbench-sidebar-splitter:focus-visible::after",
  ".workbench--editor .workbench-sidebar-splitter:hover::after",
  ".workbench--editor .workbench__body",
] as const;

const preQ05RawRadii = [
  "src/features/chat/acp/acp-activity-cards.css:7 0",
  "src/styles/agent-chat-panes.css:26 2px",
  "src/styles/agent-chat-panes.css:148 50%",
  "src/styles/changed-files-summary.css:8 0",
  "src/styles/chat-navigation-surface.css:7 5px",
  "src/styles/chat-navigation-surface.css:27 7px",
  "src/styles/chat-navigation-surface.css:47 5px 0 0 5px",
  "src/styles/chat-navigation-surface.css:78 5px",
  "src/styles/chat-navigation-surface.css:168 5px",
  "src/styles/chat-navigation-surface.css:263 50%",
  "src/styles/chat-navigation-surface.css:272 50%",
  "src/styles/chat-navigation-surface.css:321 7px",
  "src/styles/chat-navigation-surface.css:345 50%",
  "src/styles/diff-surface.css:140 7px",
  "src/styles/editor-activity-bar.css:79 0",
  "src/styles/project-navigation-surface.css:119 0",
  "src/styles/project-navigation-surface.css:230 5px",
  "src/styles/repository-sidebar-controls.css:39 5px",
  "src/styles/repository-sidebar-controls.css:138 4px",
  "src/styles/repository-sidebar-controls.css:197 7px",
  "src/styles/repository-sidebar-controls.css:437 5px",
  "src/styles/shared-controls.css:107 4px",
  "src/styles/source-control-changes.css:112 5px",
  "src/styles/source-control-changes.css:154 5px",
  "src/styles/workbench-chrome.css:235 0",
  "src/workbench/workbench-window-mode.css:102 2px",
] as const;

const preQ05RawColors = [
  "src/styles/file-editor-surface.css:65 #4c8dff",
  "src/styles/file-editor-surface.css:69 #e2b66b",
  "src/styles/file-editor-surface.css:73 #ef9a9a",
  "src/styles/file-editor-surface.css:84 #d7c078",
  "src/styles/file-editor-surface.css:93 #7aa9ff",
  "src/styles/source-control-changes.css:54 #fff",
] as const;

type CssInventory = {
  duplicateSelectors: unknown[];
  parseErrors: string[];
  rawColors: unknown[];
  rawRadii: unknown[];
  rawZIndexes: unknown[];
};

async function productionInventory(): Promise<CssInventory> {
  const discoveryUrl = pathToFileURL(
    resolve("scripts/css-highlight-policy.mjs"),
  ).href;
  const policyUrl = pathToFileURL(
    resolve("scripts/css-hygiene-policy.mjs"),
  ).href;
  const discovery = (await import(discoveryUrl)) as {
    discoverSourceFilesWithCssBound: (
      root: string,
    ) => Promise<{ cssFiles: string[]; exceededCssFileLimit: boolean }>;
  };
  const policy = (await import(policyUrl)) as {
    analyzeCssInventory: (
      files: Array<{ relativePath: string; source: string }>,
    ) => CssInventory;
  };
  const source = await discovery.discoverSourceFilesWithCssBound(process.cwd());
  expect(source.exceededCssFileLimit).toBe(false);
  const files = await Promise.all(
    source.cssFiles.map(async (relativePath) => ({
      relativePath: relativePath.replaceAll("\\", "/"),
      source: await readFile(resolve(relativePath), "utf8"),
    })),
  );
  return policy.analyzeCssInventory(files);
}

describe("production CSS hygiene", () => {
  it("freezes the parser-measured pre-Q-05 migration inventory", () => {
    expect(preQ05DuplicateSelectors).toHaveLength(32);
    expect(new Set(preQ05DuplicateSelectors).size).toBe(32);
    expect(preQ05RawRadii).toHaveLength(26);
    expect(new Set(preQ05RawRadii).size).toBe(26);
    expect(preQ05RawColors).toHaveLength(6);
    expect(new Set(preQ05RawColors).size).toBe(6);
  });

  it("keeps all production selectors, radii, and colors on one tokenized authority", async () => {
    const inventory = await productionInventory();

    expect(inventory.parseErrors).toEqual([]);
    expect(inventory.duplicateSelectors).toEqual([]);
    expect(inventory.rawRadii).toEqual([]);
    expect(inventory.rawColors).toEqual([]);
    expect(inventory.rawZIndexes).toEqual([]);
  });

  it("stacks modals above every popover and alerts above modals", async () => {
    const tokens = await readFile(resolve("src/styles/global.css"), "utf8");
    const layer = (name: string): number => {
      const match = new RegExp(`--z-${name}:\\s*(-?\\d+);`, "u").exec(tokens);
      if (!match) throw new Error(`missing --z-${name}`);
      return Number(match[1]);
    };
    const order = [
      "behind",
      "raised",
      "sticky",
      "floating",
      "sidebar-overlay",
      "overlay-backdrop",
      "overlay",
      "popover",
      "popover-stacked",
      "popover-card",
      "modal",
      "alert",
    ].map(layer);

    expect(order).toEqual([...order].sort((left, right) => left - right));
    expect(new Set(order).size).toBe(order.length);
  });
});
