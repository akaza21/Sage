import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

describe("Composer Project chip responsive seam", () => {
  it("fits seven 24px controls including Add in the 184px compact toolbar", async () => {
    const css = await readFile(
      "src/styles/chat-container-controls.css",
      "utf8",
    );
    const compact = css
      .split("@container composer (max-width: 280px)")
      .slice(1)
      .join("");
    expect(compact).toContain(".composer .composer__plus,");
    expect(compact).not.toMatch(/(?:min-width|width):\s*20px/);
    expect(7 * 24 + 2 * 4 + 3 * 2).toBeLessThanOrEqual(184);
  });

  it.each([128, 184, 252, 259, 259.5, 260, 280, 320, 321, 400, 401])(
    "keeps toolbar actions in one row down to 184px at %ipx composer content width",
    async (width) => {
      const narrow = await readFile(
        "src/styles/chat-container-controls.css",
        "utf8",
      );
      const surface = await readFile("src/styles/composer-surface.css", "utf8");
      const harness = await readFile("src/styles/harness-picker.css", "utf8");
      const model = await readFile(
        "src/styles/composer-model-picker.css",
        "utf8",
      );
      const style = document.createElement("style");
      style.textContent =
        surface +
        harness +
        model +
        narrow.replace(
          /@container composer \((max-width:|width <) (\d+)px\)\s*\{([\s\S]*?)\n\}/g,
          (_query, operator, breakpoint, rules) =>
            (
              operator === "width <"
                ? width < Number(breakpoint)
                : width <= Number(breakpoint)
            )
              ? rules
              : "",
        );
      const composer = document.createElement("form");
      composer.className = "composer";
      composer.innerHTML =
        '<div class="composer__toolbar"><div class="composer__controls"><label class="composer__harness composer__control-trigger"></label><div class="composer__model-group"></div></div><button class="composer__mic"></button></div>';
      document.head.append(style);
      document.body.append(composer);
      try {
        const wrapped = width < 182;
        expect(
          getComputedStyle(composer.querySelector(".composer__model-group")!)
            .minWidth,
        ).toBe(width <= 280 ? "24px" : width <= 320 ? "28px" : "60px");
        expect(
          getComputedStyle(composer.firstElementChild!).gridTemplateAreas,
        ).toBe(
          wrapped
            ? '"controls controls controls" "mic meter send"'
            : '"controls mic meter send"',
        );
        expect(
          getComputedStyle(composer.querySelector(".composer__controls")!)
            .flexWrap,
        ).toBe(wrapped ? "wrap" : "nowrap");
        if (width <= 400) {
          expect(
            getComputedStyle(composer.querySelector(".composer__model-group")!)
              .flexWrap,
          ).toBe("nowrap");
        }
        if (wrapped) {
          expect(
            getComputedStyle(composer.querySelector(".composer__mic")!)
              .justifySelf,
          ).toBe("end");
        }
      } finally {
        composer.remove();
        style.remove();
      }
    },
  );
});
