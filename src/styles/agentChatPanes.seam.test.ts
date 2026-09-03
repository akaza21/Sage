import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Agent Chat panes stylesheet seam", () => {
  it("owns compact split-pane identity without leaking back into the mode shell", async () => {
    const [entry, panesCss, modeCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-chat-panes.css"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/agent-chat-panes\.css["'];/);
    expect(modeCss).not.toMatch(/^\.agent-chat-panes\b/m);
    expect(modeCss).not.toMatch(/^\.agent-chat-pane\b/m);
    expect(panesCss).toMatch(
      /\.agent-chat-pane\s*\{[^}]*grid-template-rows:\s*32px minmax\(0, 1fr\);/s,
    );
    expect(panesCss).toMatch(
      /\.agent-chat-pane--focused \.agent-chat-pane__header\s*\{[^}]*box-shadow:\s*inset 2px 0 0 var\(--color-accent\);/s,
    );
    expect(panesCss).toMatch(
      /\.agent-chat-pane__title\s*\{[^}]*min-width:\s*0;[^}]*text-overflow:\s*ellipsis;/s,
    );
    expect(panesCss).toContain(".agent-chat-chip--failed");
    expect(panesCss).toMatch(
      /\.agent-chat-panes--split \.composer__shortcut\s*\{[^}]*display:\s*none;/s,
    );
    expect(modeCss).not.toContain(".composer__shortcut");
    expect(panesCss).toMatch(
      /@media \(forced-colors: active\)[\s\S]*?\.agent-chat-pane--focused \.agent-chat-pane__header\s*\{[^}]*outline:\s*2px solid Highlight;/,
    );
  });
});
