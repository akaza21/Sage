import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

import { describe, expect, it } from "vitest";

const globalSource = readFileSync(
  resolve(process.cwd(), "src/styles/global.css"),
  "utf8",
);
const composerSource = readFileSync(
  resolve(process.cwd(), "src/styles/composer-surface.css"),
  "utf8",
);
const rootBlock = globalSource.match(/:root\s*\{(?<declarations>[\s\S]*?)\n\}/)
  ?.groups?.declarations;

describe("sizing scale tokens", () => {
  it("keeps the composer card at the sixteen-pixel radius token", () => {
    expect(rootBlock).toMatch(/--radius-xl:\s*16px;/);
    expect(composerSource).toMatch(
      /\.composer\s*\{[^}]*border-radius:\s*var\(--radius-xl\);/s,
    );
  });

  it("freezes the type scale at a twelve-pixel floor", () => {
    expect(rootBlock).toMatch(/--font-size-xs:\s*12px;/);
    expect(rootBlock).toMatch(/--font-size-sm:\s*13px;/);
    expect(rootBlock).toMatch(/--font-size-md:\s*14px;/);
  });

  it("freezes icon sizes for inline and dense rows", () => {
    expect(rootBlock).toMatch(/--icon-size-sm:\s*14px;/);
    expect(rootBlock).toMatch(/--icon-size-md:\s*16px;/);
  });

  it("freezes row, tab and control heights", () => {
    expect(rootBlock).toMatch(/--row-height-compact:\s*30px;/);
    expect(rootBlock).toMatch(/--row-height-default:\s*32px;/);
    expect(rootBlock).toMatch(/--row-height-emphasis:\s*34px;/);
    expect(rootBlock).toMatch(/--tab-height:\s*36px;/);
    expect(rootBlock).toMatch(/--control-height-compact:\s*28px;/);
    expect(rootBlock).toMatch(/--control-height-default:\s*32px;/);
  });

  it("freezes the four-pixel spacing scale", () => {
    expect(rootBlock).toMatch(/--space-1:\s*4px;/);
    expect(rootBlock).toMatch(/--space-2:\s*8px;/);
    expect(rootBlock).toMatch(/--space-3:\s*12px;/);
    expect(rootBlock).toMatch(/--space-4:\s*16px;/);
    expect(rootBlock).toMatch(/--space-6:\s*24px;/);
    expect(rootBlock).toMatch(/--space-8:\s*32px;/);
  });

  it("never references a custom property that no stylesheet defines", () => {
    const sources = collectSources(resolve(process.cwd(), "src"));
    const defined = new Set<string>();
    const referenced = new Map<string, string>();
    for (const file of sources) {
      const source = readFileSync(file, "utf8");
      for (const match of source.matchAll(/(--[a-z0-9-]+)"?\s*:/g)) {
        defined.add(match[1]);
      }
      if (!file.endsWith(".css")) continue;
      for (const match of source.matchAll(/var\(\s*(--[a-z0-9-]+)/g)) {
        if (!referenced.has(match[1])) referenced.set(match[1], file);
      }
    }
    const dangling = [...referenced]
      .filter(([token]) => !defined.has(token))
      .map(([token, file]) => `${token} in ${file}`);
    expect(dangling).toEqual([]);
  });

  it("freezes a thin neutral scrollbar that brightens on hover", () => {
    expect(rootBlock).toMatch(/--scrollbar-size:\s*8px;/);
    expect(rootBlock).toMatch(/--scrollbar-thumb:\s*#3a3a3a;/);
    expect(rootBlock).toMatch(/--scrollbar-thumb-hover:\s*#4a4a4a;/);
    expect(globalSource).toMatch(
      /\*::-webkit-scrollbar\s*\{[^}]*width:\s*var\(--scrollbar-size\);/s,
    );
    expect(globalSource).toMatch(
      /\*::-webkit-scrollbar-thumb:hover\s*\{[^}]*background:\s*var\(--scrollbar-thumb-hover\);/s,
    );
  });

  it("keeps every stylesheet above the twelve-pixel type floor", () => {
    const offenders: string[] = [];
    for (const file of [
      "src/styles/global.css",
      "src/styles/agent-right-pane.css",
      "src/styles/agent-chat-panes.css",
      "src/styles/chat-navigation-surface.css",
      "src/styles/problems-outline-surfaces.css",
      "src/features/search/workspace-search.css",
      "src/features/chat/acp/acp-activity-cards.css",
      "src/features/chat/acp/acp-chat.css",
    ]) {
      const source = readFileSync(resolve(process.cwd(), file), "utf8");
      if (/font-size:\s*(?:[0-9]|1[01])px/.test(source)) offenders.push(file);
    }
    expect(offenders).toEqual([]);
  });

  it("routes scrollbar colour through the neutral tokens only", () => {
    expect(globalSource).not.toMatch(/scrollbar-color:\s*var\(--color-accent/);
    expect(rootBlock).toMatch(/--scrollbar-thumb:\s*#[0-9a-f]{6};/);
  });

  it("leaves the standard scrollbar properties unset so the eight-pixel webkit scrollbar is the one that ships", () => {
    expect(globalSource).not.toMatch(/^\s*scrollbar-width:/m);
    expect(globalSource).not.toMatch(
      /^\s*scrollbar-color:\s*var\(--scrollbar-thumb\)/m,
    );
  });

  it("anchors inherited text to the fourteen-pixel body size", () => {
    expect(globalSource).toMatch(
      /\nbody\s*\{[^}]*font-size:\s*var\(--font-size-md\);/s,
    );
  });
});

function collectSources(directory: string): string[] {
  const found: string[] = [];
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    if (statSync(path).isDirectory()) {
      found.push(...collectSources(path));
      continue;
    }
    if (/\.(?:css|ts|tsx)$/.test(path)) found.push(path);
  }
  return found;
}
