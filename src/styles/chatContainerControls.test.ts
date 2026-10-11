import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

async function containerDisplay(selector: string, width: number) {
  const css = await readFile("src/styles/chat-container-controls.css", "utf8");
  const root = document.createElement("div");
  root.className = "composer";
  root.innerHTML =
    '<div class="composer__controls"><button><span class="composer__access-text"></span></button><label class="composer__harness"><button><span class="searchable-picker__trigger-label"></span></button></label><button><svg class="composer__model-icon"></svg><span class="composer__model-label"></span></button></div>';
  const element = root.querySelector(selector);
  if (!element) throw new Error(`Missing control fixture: ${selector}`);
  let display = "inline";
  const applyRules = (source: string) => {
    for (const rule of source.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      if (
        !rule[1]
          .split(",")
          .some((candidate) => element.matches(candidate.trim()))
      )
        continue;
      const value = rule[2].match(/display:\s*([^;]+);/);
      if (value) display = value[1];
    }
  };
  applyRules(css.slice(0, css.indexOf("@container")));
  for (const query of css.matchAll(
    /@container composer \(max-width: (\d+)px\)\s*\{([\s\S]*?)\n\}/g,
  )) {
    if (width > Number(query[1])) continue;
    applyRules(query[2]);
  }
  return display;
}

describe("Chat container control breakpoints", () => {
  it("reflows goal and header chrome with their chat container, not the window", async () => {
    const css = await readFile(
      "src/styles/chat-container-controls.css",
      "utf8",
    );
    expect(css).toMatch(
      /@container chat-center \(max-width: 400px\)[\s\S]*\.agent-task-header\s*\{[^}]*flex-wrap:\s*wrap;/,
    );
    expect(css).toMatch(
      /@container composer \(max-width: 280px\)[\s\S]*\.chat-goal-strip__row\s*\{[^}]*height:\s*auto;[^}]*flex-wrap:\s*wrap;/,
    );
    expect(css).not.toMatch(/@media\s*\(max-width:/);
  });
  it.each([
    [521, "inline", "inline"],
    [520, "none", "inline"],
    [401, "none", "inline"],
    [400, "none", "none"],
    [180, "none", "none"],
  ])(
    "at %ipx collapses access text before harness/model text",
    async (width, access, identity) => {
      expect(await containerDisplay(".composer__access-text", width)).toBe(
        access,
      );
      expect(
        await containerDisplay(
          ".composer__harness .searchable-picker__trigger-label",
          width,
        ),
      ).toBe(identity);
      expect(await containerDisplay(".composer__model-label", width)).toBe(
        identity,
      );
    },
  );

  it.each([
    [401, "none"],
    [400, "block"],
    [180, "block"],
  ])(
    "declares the model icon display at %ipx as %s",
    async (width, display) => {
      expect(await containerDisplay(".composer__model-icon", width)).toBe(
        display,
      );
    },
  );
});
