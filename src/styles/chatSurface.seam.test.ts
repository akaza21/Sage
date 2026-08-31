import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Chat surface stylesheet seam", () => {
  it("loads primary Chat rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/chat-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(/import ["']\.\/styles\/chat-surface\.css["'];/);
    expect(shellCss).not.toMatch(/^\.chat-surface\b/m);
    expect(shellCss).not.toMatch(/^\.message-list\b/m);
    expect(surfaceCss).toMatch(/^\.chat-surface\b/m);
    expect(surfaceCss).toMatch(/^\.edit-summary\b/m);
  });

  it("keeps the Chat header and transcript within a narrow dual-pane width", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/chat-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(/\.surface-header\s*\{[^}]*min-width:\s*0;/s);
    expect(surfaceCss).toMatch(
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.surface-header\s*\{[^}]*flex-wrap:\s*wrap;/,
    );
    expect(surfaceCss).toMatch(
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.session-controls\s*\{[^}]*flex-wrap:\s*wrap;/,
    );
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
      /@media \(max-width: 640px\)\s*\{[\s\S]*?\.session-controls\s*\{[^}]*width:\s*100%;/,
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
