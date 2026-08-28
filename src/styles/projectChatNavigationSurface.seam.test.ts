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
    expect(projectCss).toMatch(/^\.chat-move-popover\b/m);
    expect(chatCss).toMatch(/^\.project-chat-row\b/m);
    expect(chatCss).toMatch(/^\.project-chat-edit\b/m);
  });

  it("keeps the Agent rail's projects and chats compact", async () => {
    const chatCss = await readFile(
      resolve("src/styles/chat-navigation-surface.css"),
      "utf8",
    );
    const projectCss = await readFile(
      resolve("src/styles/project-navigation-surface.css"),
      "utf8",
    );

    expect(chatCss).toMatch(
      /\.project-chat-row\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+repeat\(4,\s*24px\);/s,
    );
    expect(chatCss).toMatch(/\.project-chat\s*\{[^}]*min-height:\s*36px;/s);
    expect(projectCss).toMatch(
      /\.project-sidebar__actions\s*\{[^}]*padding:\s*6px\s+8px;/s,
    );
    expect(projectCss).toMatch(/\.project-section\s*\{[^}]*padding:\s*8px/s);
    expect(projectCss).toMatch(/\.project-row\s*\{[^}]*height:\s*28px;/s);
  });
});
