import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

describe("Chat container control breakpoints", () => {
  it("truncates rather than removes the project name in the header menu", async () => {
    const header = await readFile("src/styles/agent-task-header.css", "utf8");
    expect(header).toMatch(
      /\.agent-header-more__project > span\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
    );
    expect(header).not.toMatch(
      /\.agent-header-more__project > span\s*\{[^}]*display:\s*none;/s,
    );
  });

  it.each([
    ["composer__context-usage", "src/styles/composer-context.css"],
    ["composer__attachment-remove", "src/styles/composer-surface.css"],
  ])("reserves at least a 24x24 hit target for %s", async (className, path) => {
    const style = document.createElement("style");
    style.textContent = await readFile(path, "utf8");
    const control = document.createElement("button");
    control.className = className;
    document.head.append(style);
    document.body.append(control);
    try {
      const computed = getComputedStyle(control);
      expect(Number.parseFloat(computed.width)).toBeGreaterThanOrEqual(24);
      expect(Number.parseFloat(computed.height)).toBeGreaterThanOrEqual(24);
    } finally {
      control.remove();
      style.remove();
    }
  });

  it.each([
    [580, 24],
    [521, 24],
    [520, 24],
    [400, 24],
  ])(
    "keeps the options button at %ipx container width exactly %ipx wide",
    async (width, expected) => {
      const style = document.createElement("style");
      const compact = await readFile(
        "src/styles/chat-container-controls.css",
        "utf8",
      );
      style.textContent =
        (await readFile("src/styles/chat-surface.css", "utf8")) +
        compact.replace(
          /@container composer \(max-width: (\d+)px\)\s*\{([\s\S]*?)\n\}/g,
          (_query, breakpoint, rules) =>
            width <= Number(breakpoint) ? rules : "",
        );
      const composer = document.createElement("form");
      composer.className = "composer";
      composer.innerHTML = '<button class="composer__plus"></button>';
      document.head.append(style);
      document.body.append(composer);
      try {
        expect(getComputedStyle(composer.firstElementChild!).width).toBe(
          expected + "px",
        );
        expect(getComputedStyle(composer.firstElementChild!).height).toBe(
          "28px",
        );
      } finally {
        composer.remove();
        style.remove();
      }
    },
  );

  it("compacts header titles and reflows goals with their chat container, not the window", async () => {
    const css = await readFile(
      "src/styles/chat-container-controls.css",
      "utf8",
    );
    const header = await readFile("src/styles/agent-task-header.css", "utf8");
    expect(header).toContain("@container chat-center (max-width: 400px)");
    expect(header).toMatch(
      /\.agent-task-header__title\s*\{[^}]*min-width:\s*3em;/s,
    );
    expect(header).not.toMatch(/font-size:\s*0;/);
    expect(css).toMatch(
      /@container composer \(max-width: 280px\)[\s\S]*\.chat-goal-strip__row\s*\{[^}]*height:\s*auto;[^}]*flex-wrap:\s*wrap;/,
    );
    expect(css).not.toMatch(/@media\s*\(max-width:/);
  });
});
