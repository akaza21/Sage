import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Project and chat navigation stylesheet seams", () => {
  it("loads navigation rules from focused stylesheets, not the shell stylesheet", async () => {
    const [entry, shellCss, projectCss, chatCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/project-navigation-surface.css"), "utf8"),
      readFile(resolve("src/styles/chat-navigation-surface.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/project-navigation-surface\.css["'];/,
    );
    expect(entry).toMatch(
      /import ["']\.\/styles\/chat-navigation-surface\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.project-sidebar\b/m);
    expect(shellCss).not.toMatch(/^\.project-chat-row\b/m);
    expect(projectCss).toMatch(/^\.project-sidebar\b/m);
    expect(projectCss).toMatch(/\.project-sidebar__drawer\b/);
    expect(chatCss).toMatch(/^\.project-chat-row\b/m);
    expect(chatCss).toMatch(/^\.project-chat-actions__menu\b/m);
    expect(chatCss).toMatch(/^\.project-chat-edit\b/m);
  });

  it("keeps the Agent rail flat, compact, and title-safe", async () => {
    const chatCss = await readFile(
      resolve("src/styles/chat-navigation-surface.css"),
      "utf8",
    );
    const projectCss = await readFile(
      resolve("src/styles/project-navigation-surface.css"),
      "utf8",
    );

    expect(chatCss).toMatch(
      /\.project-chat-row\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+minmax\(28px,\s*auto\);/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat-row\s*\{[^}]*grid-template-rows:\s*auto auto;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat\s*\{[^}]*min-height:\s*var\(--row-height-default\);/s,
    );
    expect(chatCss).not.toMatch(
      /\.project-chat\s*\{[^}]*[^-]height:\s*var\(--row-height-default\);/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat\s*\{[^}]*grid-column:\s*1 \/ -1;[^}]*grid-row:\s*1;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat__context\s*\{[^}]*grid-column:\s*1;[^}]*grid-row:\s*2;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat__trailing\s*\{[^}]*grid-row:\s*2;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat-row:hover \.project-chat__state,[\s\S]*?\.project-chat-row:focus-within \.project-chat__state\s*\{[^}]*opacity:\s*0;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat-row:hover \.project-chat__actions,[\s\S]*?\.project-chat-row:focus-within \.project-chat__actions,[\s\S]*?\.project-chat__actions\[aria-expanded="true"\]\s*\{[^}]*opacity:\s*1;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat__actions:disabled\s*\{[^}]*opacity:\s*0;/s,
    );
    expect(chatCss).toMatch(
      /\.project-chat-row:hover \.project-chat__actions:disabled,[\s\S]*?\.project-chat-row:focus-within \.project-chat__actions:disabled\s*\{[^}]*opacity:\s*0\.4;/s,
    );
    expect(chatCss.match(/^\.project-chat__working\s*\{/gm)).toHaveLength(1);
    expect(chatCss).toMatch(/@media \(prefers-reduced-motion: reduce\)/);
    expect(chatCss).toMatch(/@media \(forced-colors: active\)/);
    expect(projectCss).toMatch(
      /\.project-sidebar__actions\s*\{[^}]*padding:\s*var\(--space-3\) var\(--space-3\) var\(--space-2\);/s,
    );
    expect(projectCss).not.toMatch(/\.project-section\b/);
    expect(projectCss).toMatch(/\.project-row\s*\{[^}]*height:\s*32px;/s);
    expect(projectCss).toMatch(
      /\.project-row \.project-row__branch\s*\{[^}]*border:\s*0;/s,
    );
    expect(projectCss).toMatch(
      /\.project-row \.project-row__branch--chip\s*\{[^}]*border:\s*1px solid var\(--color-border-strong\);[^}]*border-radius:\s*var\(--radius-pill\);/s,
    );
  });

  it("uses a real compact navigation drawer instead of icon-crushing chat rows", async () => {
    const [shellCss, projectCss, chatCss] = await Promise.all([
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/project-navigation-surface.css"), "utf8"),
      readFile(resolve("src/styles/chat-navigation-surface.css"), "utf8"),
    ]);

    expect(shellCss).not.toMatch(/\.project-sidebar__actions\b/);
    expect(shellCss).not.toMatch(/\.project-chats\b/);
    expect(shellCss).not.toMatch(/\.project-empty-action\b/);
    expect(shellCss).not.toMatch(/\.project-sidebar__account\b/);
    expect(projectCss).toMatch(
      /@media \(max-width: 760px\)[\s\S]*\.project-sidebar--compact\b/,
    );
    expect(projectCss).toMatch(
      /@media \(max-width: 760px\)[\s\S]*\.project-sidebar__drawer\b/,
    );
    expect(chatCss).not.toMatch(/@media \(max-width: 760px\)/);
  });

  it("seats Settings inside the rail and leaves the brand to the title bar", async () => {
    const [projectCss, chatCss] = await Promise.all([
      readFile(resolve("src/styles/project-navigation-surface.css"), "utf8"),
      readFile(resolve("src/styles/chat-navigation-surface.css"), "utf8"),
    ]);

    expect(projectCss).not.toMatch(/\.project-sidebar__brand/);
    expect(projectCss).toMatch(
      /\.project-sidebar\s*\{[^}]*grid-template-rows:\s*auto minmax\(0,\s*1fr\)\s+auto;/s,
    );
    expect(chatCss).toMatch(
      /\.project-sidebar__account\s*\{[^}]*grid-template-columns:\s*auto minmax\(0,\s*1fr\)\s+auto;/s,
    );
    expect(chatCss).toMatch(
      /\.project-sidebar__settings\s*\{[^}]*border:\s*0;/s,
    );
  });
});
