import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Agent Chat panes stylesheet seam", () => {
  it("owns compact split-pane identity without leaking back into the mode shell", async () => {
    const [entry, panesCss, modeCss, splitControlsCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-chat-panes.css"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
      readFile(resolve("src/styles/split-chat-controls.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/agent-chat-panes\.css["'];/);
    expect(panesCss).toContain('@import "./split-chat-controls.css";');
    expect(modeCss).not.toMatch(/^\.agent-chat-panes\b/m);
    expect(modeCss).not.toMatch(/^\.agent-chat-pane\b/m);
    expect(panesCss).toMatch(
      /\.agent-chat-pane\s*\{[^}]*grid-template-rows:\s*minmax\(0, 1fr\);/s,
    );
    expect(panesCss).toMatch(
      /\.agent-chat-panes--split \.agent-chat-pane--focused\s*\{[^}]*outline:\s*1px solid var\(--color-border-strong\);[^}]*outline-offset:\s*-1px;/s,
    );
    expect(panesCss).not.toContain(".agent-chat-pane--focused .surface-header");
    expect(panesCss).toMatch(
      /\.agent-chat-pane__title\s*\{[^}]*min-width:\s*0;[^}]*text-overflow:\s*ellipsis;/s,
    );
    expect(panesCss).toContain(".agent-chat-chip--failed");
    expect(splitControlsCss).toMatch(
      /\.agent-chat-panes--split \.composer__shortcut\s*\{[^}]*display:\s*none;/s,
    );
    expect(panesCss + splitControlsCss).not.toMatch(
      /\.agent-chat-panes--split \.access-mode\s*\{[^}]*display:\s*none;/s,
    );
    expect(modeCss).not.toContain(".composer__shortcut");
    expect(panesCss).toMatch(
      /@media \(forced-colors: active\)[\s\S]*?\.agent-chat-panes--split \.agent-chat-pane--focused\s*\{[^}]*outline:\s*2px solid Highlight;[^}]*outline-offset:\s*-2px;[^}]*box-shadow:\s*none;/,
    );
  });

  it("keeps idle splitter rails at two pixels and narrows only keyboard focus", async () => {
    const [panesCss, modeCss, globalCss] = await Promise.all([
      readFile(resolve("src/styles/agent-chat-panes.css"), "utf8"),
      readFile(resolve("src/workbench/workbench-window-mode.css"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
    ]);

    for (const [css, family] of [
      [panesCss, "agent-chat-splitter"],
      [modeCss, "workbench-sidebar-splitter"],
    ] as const) {
      expect(css).toMatch(
        new RegExp(
          `\\.${family}::after\\s*\\{[^}]*left:\\s*3px;[^}]*width:\\s*2px;`,
          "s",
        ),
      );
      expect(css).toMatch(
        new RegExp(
          `\\.${family}:focus-visible::after\\s*\\{[^}]*width:\\s*var\\(--focus-ring-width\\);[^}]*background:\\s*var\\(--highlight-focus-edge\\);[^}]*box-shadow:\\s*var\\(--highlight-focus-halo-shadow\\);`,
          "s",
        ),
      );
      expect(css).toMatch(
        new RegExp(
          `@media \\(forced-colors:\\s*active\\)\\s*\\{[\\s\\S]*?\\.${family}::after\\s*\\{[^}]*width:\\s*2px;[^}]*\\}[\\s\\S]*?\\.${family}:focus-visible::after\\s*\\{[^}]*background:\\s*Highlight;[^}]*box-shadow:\\s*none;`,
          "s",
        ),
      );
    }

    expect(globalCss).toMatch(
      /@media \(forced-colors:\s*active\)[\s\S]*?--focus-ring-width:\s*2px;/,
    );
  });
});
