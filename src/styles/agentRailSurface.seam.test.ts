import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Agent rail stylesheet seam", () => {
  it("keeps the palette trigger, section controls, and active status tokenized", async () => {
    const [component, surfaceCss, chatCss] = await Promise.all([
      readFile(resolve("src/workbench/LeftSidebar.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
      readFile(resolve("src/styles/chat-navigation-surface.css"), "utf8"),
    ]);

    expect(component).toContain('aria-label="Search chats"');
    expect(component).not.toContain("<input");
    expect(surfaceCss).not.toContain(".agent-rail-search");
    expect(chatCss).toMatch(
      /\.project-chat__status-chip\s*\{[^}]*border-radius:\s*var\(--radius-pill\);/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat__status-chip--working,[\s\S]*color:\s*var\(--color-warning\);/,
    );
    expect(chatCss).toMatch(
      /@media \(forced-colors: active\)[\s\S]*\.project-chat__status-chip\s*\{[^}]*color:\s*CanvasText;[^}]*background:\s*Canvas;/s,
    );
    expect(surfaceCss).not.toMatch(/^\.project-(?:chat|row|sidebar)/m);

    expect(surfaceCss).not.toMatch(/#[\da-fA-F]{3,8}\b|rgba?\s*\(/);
  });
});
