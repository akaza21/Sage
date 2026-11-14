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
});
