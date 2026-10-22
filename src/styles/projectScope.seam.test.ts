import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

describe("Composer Project chip responsive seam", () => {
  it("keeps the split chip nonshrinking after the later composer styles load", async () => {
    const style = document.createElement("style");
    style.textContent = (
      await Promise.all(
        [
          "shared-controls.css",
          "agent-chat-panes.css",
          "split-chat-controls.css",
          "composer-surface.css",
          "harness-picker.css",
        ].map((file) => readFile(`src/styles/${file}`, "utf8")),
      )
    ).join("\n");
    const pane = document.createElement("div");
    pane.className = "agent-chat-panes--split";
    pane.innerHTML =
      '<form class="composer"><div class="composer__controls"><span class="select-control project-scope"><span class="searchable-picker"><button class="searchable-picker__trigger"><svg class="project-scope__icon"></svg><span class="searchable-picker__trigger-label">Alpha</span><svg class="searchable-picker__chevron"></svg></button></span></span></div></form>';
    document.head.append(style);
    document.body.append(pane);
    try {
      const chip = getComputedStyle(pane.querySelector(".project-scope")!);
      expect(chip.width).toBe("28px");
      expect(chip.flexShrink).toBe("0");
      const trigger = getComputedStyle(pane.querySelector("button")!);
      expect(trigger.minWidth).toBe("28px");
      expect(trigger.minHeight).toBe("28px");
      expect(
        getComputedStyle(
          pane.querySelector(".searchable-picker__trigger-label")!,
        ).display,
      ).toBe("none");
      expect(
        getComputedStyle(pane.querySelector(".searchable-picker__chevron")!)
          .display,
      ).toBe("none");
    } finally {
      pane.remove();
      style.remove();
    }
  });

  it("reserves clickable icon targets while allowing the harness to shrink", async () => {
    const surface = await readFile("src/styles/composer-surface.css", "utf8");
    const harness = await readFile("src/styles/harness-picker.css", "utf8");
    const model = await readFile(
      "src/styles/composer-model-picker.css",
      "utf8",
    );
    expect(model).toMatch(
      /\.composer__model-group \.composer__model-trigger\s*\{[^}]*min-width:\s*28px;/s,
    );
    expect(surface).toMatch(/\.project-scope\s*\{[^}]*min-width:\s*28px;/s);
    expect(surface).toMatch(
      /\.project-scope \.searchable-picker__trigger\s*\{[^}]*min-width:\s*28px;[^}]*min-height:\s*28px;/s,
    );
    expect(harness).toMatch(
      /\.composer__harness\.composer__control-trigger\s*\{[^}]*min-width:\s*28px;[^}]*flex:\s*0 1 128px;/s,
    );
    expect(model).toMatch(
      /\.composer__model-group \.composer__effort-picker\s*\{[^}]*min-width:\s*28px;[^}]*overflow:\s*hidden;/s,
    );
  });

  it("uses the same nonshrinking icon-only chip below 400px and in split panes", async () => {
    const narrow = await readFile(
      "src/styles/chat-container-controls.css",
      "utf8",
    );
    const split = (
      await readFile("src/styles/split-chat-controls.css", "utf8")
    ).replace(/\s+/g, " ");
    const compact = narrow
      .split("@container composer (max-width: 400px)")[1]
      ?.split("@container composer (max-width: 280px)")[0];
    expect(compact).toBeDefined();
    for (const [css, prefix] of [
      [compact!, ".composer"],
      [split, ".agent-chat-panes--split"],
    ]) {
      const target =
        prefix === ".composer" ? prefix : `${prefix} .composer__controls >`;
      const selector = target.replaceAll(".", "\\.");
      expect(css).toMatch(
        new RegExp(
          `${selector} \\.project-scope\\s*\\{[^}]*width:\\s*28px;[^}]*flex:\\s*none;`,
          "s",
        ),
      );
      expect(css).toContain(
        `${target} .project-scope .searchable-picker__trigger-label`,
      );
      expect(css).toContain(
        `${target} .project-scope .searchable-picker__chevron`,
      );
      expect(css).toContain(
        prefix === ".composer"
          ? `${prefix} .project-scope__label`
          : `${target} .project-scope .project-scope__label`,
      );
      expect(css).toMatch(/project-scope__label\s*\{[^}]*display:\s*none;/s);
    }
    expect(split).not.toContain(".composer__controls > .select-control {");
  });

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
        const harnessStyle = getComputedStyle(
          composer.querySelector(".composer__harness")!,
        );
        expect(harnessStyle.flexShrink).toBe("1");
        expect(harnessStyle.flexBasis).toBe(
          width <= 280 ? "24px" : width <= 400 ? "28px" : "128px",
        );
        expect(harnessStyle.minWidth).toBe(width <= 280 ? "24px" : "28px");
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
