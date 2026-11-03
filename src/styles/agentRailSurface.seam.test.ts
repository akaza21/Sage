import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Agent rail stylesheet seam", () => {
  it("reuses the shared menu classes without nesting a select", async () => {
    const [component, surfaceCss] = await Promise.all([
      // The open menu is its own lazy chunk (AgentRailFilterOptions.tsx).
      readFile(resolve("src/workbench/AgentRailFilterOptions.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
    ]);

    expect(surfaceCss).toMatch(
      /\.agent-rail-status__header\s*\{[^}]*font-size:\s*var\(--font-size-xs\);/s,
    );
    expect(component).toContain('className="menu-item"');
    expect(component).toContain('className="menu-group-label"');
    expect(component).not.toContain("<select");
    expect(component).toContain('role="menuitemradio"');
    expect(surfaceCss).not.toMatch(
      /\.agent-rail-filter__menu\s*\{[^}]*position:\s*absolute/s,
    );
  });

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

  it("keeps the rail filter trigger and menu actions on consistent targets", async () => {
    const [trigger, menu, surfaceCss] = await Promise.all([
      readFile(resolve("src/workbench/AgentRailFilterMenu.tsx"), "utf8"),
      readFile(resolve("src/workbench/AgentRailFilterOptions.tsx"), "utf8"),
      readFile(resolve("src/styles/agent-rail-surface.css"), "utf8"),
    ]);
    const component = trigger + menu;

    expect(surfaceCss).toMatch(
      /\.agent-rail-filter__trigger\s*\{[^}]*width:\s*var\(--control-height-compact\);[^}]*height:\s*var\(--control-height-compact\);/s,
    );
    expect(component).toContain("<MenuPopover");
    expect(component).toContain("trapFocus");
    expect(component).toContain('aria-haspopup="menu"');
  });
});
