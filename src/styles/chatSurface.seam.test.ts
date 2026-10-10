import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Chat surface stylesheet seam", () => {
  it("defines token-based and forced-color focus boundaries for the home project chip", async () => {
    const css = await readFile(
      resolve("src/features/chat/chat-home.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.chat-home \.project-scope:focus-within\s*\{[^}]*outline:\s*var\(--focus-ring\);[^}]*outline-offset:\s*var\(--focus-ring-offset\)/s,
    );
    expect(css).toMatch(
      /@media \(forced-colors: active\)[\s\S]*\.project-scope:focus-within\s*\{[^}]*outline-color:\s*Highlight/,
    );
  });

  it("colors independently known changed-file totals by meaning rather than sibling position", async () => {
    const css = await readFile(
      resolve("src/styles/changed-files-summary.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.acp-file-changes__additions[^}]*color:\s*var\(--color-diff-add\)/s,
    );
    expect(css).toMatch(
      /\.acp-file-changes__deletions[^}]*color:\s*var\(--color-diff-delete\)/s,
    );
    expect(css).not.toMatch(
      /\.acp-file-changes__totals span:(first|last)-child/,
    );
  });

  it("loads primary Chat rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss, changedFilesCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/chat-surface.css"), "utf8"),
      readFile(resolve("src/styles/changed-files-summary.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/chat-surface\.css["'];/);
    expect(surfaceCss).not.toMatch(/font-size:\s*15px/);
    expect(shellCss).not.toMatch(/^\.chat-surface\b/m);
    expect(shellCss).not.toMatch(/^\.message-list\b/m);
    expect(surfaceCss).toMatch(/^\.chat-surface\b/m);
    expect(entry).toMatch(
      /import ["']\.\/styles\/changed-files-summary\.css["'];/,
    );
    expect(surfaceCss).not.toMatch(/^\.edit-summary\b/m);
    expect(changedFilesCss).toMatch(/^\.edit-summary\b/m);
    expect(changedFilesCss).toMatch(/^\.acp-file-changes\b/m);
    expect(changedFilesCss).not.toMatch(/edit-summary__source-control/);
  });

  it("keeps the Chat action row compact without a narrow wrapping jump", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/chat-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.surface-header\s*\{[^}]*min-width:\s*0;[^}]*min-height:\s*var\(--tab-height\);[^}]*max-height:\s*var\(--tab-height\);/s,
    );
    expect(surfaceCss).not.toMatch(/\.surface-header\s*\{[^}]*flex-wrap:/s);
  });

  it("gives header controls a shared target and room to wrap", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/chat-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.session-controls\s*\{[^}]*gap:\s*var\(--space-1\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.session-controls\s+\.button--compact\s*\{[^}]*min-height:\s*var\(--control-height-compact\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.session-controls\s+\.button--compact\s*\{[^}]*background:\s*transparent;/s,
    );
    expect(surfaceCss).not.toMatch(
      /\.session-state\s*\{[^}]*background:\s*var\(--color-surface\);/s,
    );
  });

  it("keeps Chat reading text distinct from compact chrome", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/chat-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.message p\s*\{[^}]*font-size:\s*var\(--font-size-md\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.message__meta\s*\{[^}]*font-size:\s*var\(--font-size-xs\);/s,
    );
    expect(surfaceCss).not.toMatch(/\.session-tab\s*\{/);
  });
});
