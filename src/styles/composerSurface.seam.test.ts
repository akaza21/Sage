import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

async function composerRules() {
  const files = [
    "surface.css",
    "global.css",
    "shared-controls.css",
    "status-surfaces.css",
    "chat-surface.css",
    "composer-surface.css",
    "composer-image.css",
    "composer-context.css",
  ];
  return (
    await Promise.all(
      files.map((file) => readFile(resolve("src/styles", file), "utf8")),
    )
  ).join("\n");
}

describe("Composer surface stylesheet seam", () => {
  it("keeps queued rows inside the dock flow even when the edit summary appears", async () => {
    const style = document.createElement("style");
    style.textContent = await readFile(
      "src/styles/queued-messages.css",
      "utf8",
    );
    const dock = document.createElement("form");
    dock.className = "composer";
    dock.innerHTML =
      '<div class="chat-edit-summary-zone"></div><div class="composer__queued"><div class="composer__queued-row">Follow up</div></div>';
    document.head.append(style);
    document.body.append(dock);
    try {
      const queue = dock.querySelector(".composer__queued")!;
      for (const summary of ["", "1 file changed"]) {
        dock.firstElementChild!.textContent = summary;
        expect(getComputedStyle(queue).position).toBe("static");
        expect(getComputedStyle(queue.firstElementChild!).backgroundColor).toBe(
          "rgba(0, 0, 0, 0)",
        );
        expect(getComputedStyle(queue).maxHeight).toBe("min(320px, 40vh)");
      }
    } finally {
      dock.remove();
      style.remove();
    }
  });

  it("keeps the dock edit slot unpainted and scrollable", async () => {
    const style = document.createElement("style");
    style.textContent = await readFile("src/styles/chat-surface.css", "utf8");
    const slot = document.createElement("div");
    slot.className = "chat-edit-summary-zone";
    slot.textContent = "1 file changed";
    document.head.append(style);
    document.body.append(slot);
    try {
      expect(getComputedStyle(slot).overflowY).toBe("auto");
      expect(getComputedStyle(slot).backgroundColor).toBe("rgba(0, 0, 0, 0)");
      expect(getComputedStyle(slot).minHeight).toBe("0px");
    } finally {
      slot.remove();
      style.remove();
    }
  });

  it("lets expanded edit disclosures shrink and scroll only in their Agent grid row", async () => {
    const css = await readFile("src/styles/chat-surface.css", "utf8");
    const style = document.createElement("style");
    style.textContent = css;
    const agent = document.createElement("section");
    agent.className = "chat-surface--agent";
    agent.innerHTML =
      '<div class="chat-edit-summary-zone"><section class="acp-file-changes"></section></div>';
    const foreign = document.createElement("div");
    foreign.className = "chat-edit-summary-zone";
    document.head.append(style);
    document.body.append(agent, foreign);
    try {
      const zone = agent.firstElementChild!;
      expect(getComputedStyle(zone).overflowY).toBe("auto");
      expect(getComputedStyle(foreign).gridArea).not.toBe("edits");
      expect(getComputedStyle(zone).minHeight).toBe("0px");
      expect(getComputedStyle(zone).display).toBe("block");
    } finally {
      style.remove();
      agent.remove();
      foreign.remove();
    }
  });

  it("keeps transcript and composer in their named rows when notices and follow controls mount", async () => {
    const style = document.createElement("style");
    style.textContent =
      (await readFile("src/styles/chat-follow-end.css", "utf8")) +
      (await readFile("src/styles/chat-surface.css", "utf8"));
    const surface = document.createElement("section");
    surface.className = "chat-surface chat-surface--agent";
    document.head.append(style);
    document.body.append(surface);
    try {
      for (const extraRows of [false, true]) {
        surface.innerHTML =
          (extraRows
            ? '<div class="composer__notice">Account notice</div>'
            : "") +
          '<div class="message-list">Transcript</div>' +
          (extraRows
            ? '<button class="chat-follow-end">Jump to latest</button>'
            : "") +
          '<div class="chat-dock"><form class="composer"></form></div>';
        expect(
          getComputedStyle(surface.querySelector(".message-list")!).gridArea,
        ).toBe("transcript");
        expect(
          getComputedStyle(surface.querySelector(".chat-dock")!).gridArea,
        ).toBe("composer");
        if (extraRows) {
          expect(
            getComputedStyle(surface.querySelector(".composer__notice")!)
              .gridArea,
          ).toBe("notice");
          // Jump to latest overlays the transcript instead of taking a row.
          const jump = getComputedStyle(
            surface.querySelector(".chat-follow-end")!,
          );
          expect(jump.gridArea).toBe("transcript");
          expect(jump.alignSelf).toBe("end");
        }
      }
    } finally {
      surface.remove();
      style.remove();
    }
  });

  it("keeps natural-width composer controls available when an artifact pane narrows Chat", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.select-control\s*\{[^}]*max-width:\s*none;[^}]*grid-template-columns:\s*auto\s+minmax\(0,\s*1fr\);/s,
    );
  });

  it("keeps access as an unbordered label until the access-menu slice", async () => {
    const css = await composerRules();
    const globalCss = await readFile(resolve("src/styles/global.css"), "utf8");
    const accessRules = css.matchAll(
      /\.composer__access-label(?:--write)?\s*\{([^}]*)\}/gs,
    );
    expect([...accessRules].map((rule) => rule[1])).toEqual([
      expect.not.stringMatching(
        /\b(?:padding|border|border-radius|background):/,
      ),
      expect.not.stringMatching(/\b(?:border-color|background):/),
    ]);
    expect(globalCss).not.toMatch(
      /--composer-access-(?:border|fill|write-border|write-fill):/,
    );
  });
});
