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

  it("caps the composer row to leave transcript space and scrolls upper content at every width", async () => {
    const surface = await composerRules();
    expect(surface).toMatch(
      /\.chat-surface--agent\s*\{[^}]*grid-template-rows:\s*auto\s+minmax\(0,\s*1fr\)\s+fit-content\(\s*60%\s*\);/s,
    );
    expect(surface).toMatch(
      /\.composer\s*\{[^}]*display:\s*flex;[^}]*min-height:\s*0;[^}]*max-height:\s*100%;[^}]*flex-direction:\s*column;/s,
    );
    const compact = await readFile(
      "src/styles/chat-container-controls.css",
      "utf8",
    );
    expect(compact).toMatch(
      /@container composer \(max-width: 520px\)[\s\S]*\.composer__body\s*\{[^}]*scrollbar-gutter:\s*stable;/s,
    );
    expect(surface).toMatch(
      /\.composer__body\s*\{[^}]*display:\s*block;[^}]*min-height:\s*var\(--composer-input-min-block-size\);[^}]*overflow-y:\s*auto;/s,
    );
    expect(surface).toMatch(/\.composer__toolbar\s*\{[^}]*flex:\s*none;/s);
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
          '<form class="composer"></form>';
        expect(
          getComputedStyle(surface.querySelector(".message-list")!).gridArea,
        ).toBe("transcript");
        expect(
          getComputedStyle(surface.querySelector(".composer")!).gridArea,
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

  it("uses a whole-pixel 16px model heading line box", async () => {
    const css = await readFile(
      resolve("src/styles/composer-model-picker.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.composer-model-picker__title\s*\{[^}]*line-height:\s*16px;/s,
    );
  });

  it("uses reference-width model menus with compact options, bounded scrolling and system forced colors", async () => {
    const css = await readFile(
      resolve("src/styles/composer-model-picker.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.composer__model-menu\.menu-popover\s*\{[^}]*width:\s*min\(232px, calc\(100vw - 32px\)\);[^}]*max-height:\s*min\(288px, calc\(100vh - 16px\)\);/s,
    );
    expect(css).toMatch(
      /\.composer-model-picker__list\s*\{[^}]*min-height:\s*0;[^}]*overflow-y:\s*auto;/s,
    );
    expect(css).toMatch(
      /\.composer-model-picker \.searchable-picker__option\s*\{[^}]*min-height:\s*var\(--control-height-compact\);/s,
    );
    expect(css).not.toMatch(/favorites|__favorite|__header input/);
    expect(css.slice(css.indexOf("@media (forced-colors: active)"))).toMatch(
      /background:\s*Canvas;/,
    );
    expect(css.slice(css.indexOf("@media (forced-colors: active)"))).toMatch(
      /background:\s*Highlight;/,
    );
  });
  it("gives single-line harness rows bounded scrolling and visible keyboard focus", async () => {
    const css = await readFile(
      resolve("src/styles/harness-picker.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.composer__harness-menu > \[role="listbox"\]\s*\{[^}]*min-height:\s*0;[^}]*flex:\s*1 1 auto;[^}]*overflow-y:\s*auto;/s,
    );
    expect(css).toMatch(
      /\.composer__harness-menu \.searchable-picker__option\s*\{[^}]*min-height:\s*32px;/s,
    );
    expect(css).not.toMatch(/\.searchable-picker__option\[data-active/);
    expect(css).toMatch(
      /\.composer__harness-menu \.searchable-picker__option:focus-visible\s*\{[^}]*background:\s*var\(--composer-control-hover\);/s,
    );
  });

  it("truncates the model label within its own shrinkable box instead of clipping the trigger's leading edge", async () => {
    const css = await readFile(
      resolve("src/styles/composer-model-picker.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.composer__model-label\s*\{[^}]*min-width:\s*0;[^}]*overflow:\s*hidden;[^}]*text-align:\s*left;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
    );
    expect(css).toMatch(
      /\.composer__model-group \.composer__model-trigger\s*\{[^}]*justify-content:\s*flex-start;/s,
    );
    expect(css).toMatch(
      /\.composer__model-trigger > svg\s*\{[^}]*width:\s*var\(--icon-size-md\);[^}]*height:\s*var\(--icon-size-md\);[^}]*flex:\s*0 0 auto;/s,
    );
  });
  it("keeps Add menu labels on the first row with wrapped secondary descriptions", async () => {
    const css = await readFile(
      resolve("src/styles/harness-picker.css"),
      "utf8",
    );
    expect(css).toMatch(
      /\.composer__add-action > span:not\(\.composer__action-description\)\s*\{[^}]*grid-column:\s*2;[^}]*grid-row:\s*1;/s,
    );
    expect(css).toMatch(
      /\.composer__action-description\s*\{[^}]*grid-column:\s*2 \/ -1;[^}]*min-width:\s*0;[^}]*white-space:\s*normal;/s,
    );
  });
  it("reserves the full Harness trigger height instead of overlapping the prompt", async () => {
    const css = await readFile(
      resolve("src/styles/harness-picker.css"),
      "utf8",
    );
    const wrapper = css.match(
      /^\.composer__harness\.composer__control-trigger\s*\{([^}]*)\}/m,
    );
    const trigger = css.match(
      /^\.composer__harness \.searchable-picker__trigger\s*\{([^}]*)\}/m,
    );
    const height = (rule: RegExpMatchArray | null) =>
      rule?.[1].match(/(?:^|;)\s*height:\s*([^;]+);/)?.[1];
    expect(height(trigger)).toBe("var(--control-height-compact)");
    expect(trigger?.[1]).toContain("background: transparent;");
    expect(height(wrapper)).toBe(height(trigger));
  });
  it("keeps a system progress fill visible in forced colors", async () => {
    const css = await readFile(
      resolve("src/styles/composer-context.css"),
      "utf8",
    );
    const forcedColors = css.slice(
      css.indexOf("@media (forced-colors: active)"),
    );
    expect(forcedColors).toMatch(
      /\.composer__context-usage::before\s*\{[^}]*forced-color-adjust:\s*none;[^}]*background:\s*conic-gradient\(\s*Highlight\s+var\(--composer-context-usage\),\s*Canvas\s+var\(--composer-context-usage\)\s*\);/s,
    );
  });
  it("matches the Codex single-card footprint and round white action", async () => {
    const surfaceCss = await composerRules();
    expect(surfaceCss).toMatch(/--reading-measure:\s*72ch;/s);
    expect(surfaceCss).toMatch(
      /\.composer,\s*\.chat-surface > \.acp-permission-prompt\s*\{[^}]*width:\s*min\(\s*var\(--reading-measure\),/s,
    );
    expect(surfaceCss).toMatch(
      /\.surface--dock\s*\{[^}]*--surface-fill:\s*var\(--composer-fill\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.chat-surface > \.acp-permission-prompt\s*\{[^}]*border:\s*var\(--border-width\) solid var\(--composer-border\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.chat-surface > \.acp-permission-prompt\s*\{[^}]*border-radius:\s*var\(--radius-xl\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.send-button\s*\{[^}]*border-radius:\s*var\(--radius-circle\);[^}]*background:\s*var\(--color-status-foreground\);/s,
    );
  });

  it("keeps the full-width controls on one line and leaves options positioning to the shared popover", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.composer__controls\s*\{[^}]*flex-wrap:\s*nowrap;/s,
    );
    expect(surfaceCss).not.toMatch(
      /\.composer__options\s*\{[^}]*(position|top|right|bottom|left):/s,
    );
    expect(surfaceCss).toMatch(/\.menu-popover\s*\{[^}]*position:\s*fixed;/s);
    expect(surfaceCss).toMatch(
      /\.composer__toolbar\s*\{[^}]*grid-template-areas:\s*"controls\s+mic\s+meter\s+send";[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s+auto\s+auto\s+auto;/s,
    );
  });

  it("gives harness and model menus one focused popover treatment", async () => {
    const css = await composerRules();
    expect(css).toMatch(
      /\.composer__popover\s*\{[^}]*width:\s*min\(300px,\s*calc\(100vw - 32px\)\);[^}]*gap:\s*4px;[^}]*border:\s*1px solid var\(--composer-menu-border\);[^}]*color:\s*var\(--composer-control-text\);[^}]*box-shadow:\s*var\(--composer-menu-shadow\);/s,
    );
    expect(css).toMatch(
      /\.composer__popover:focus-visible\s*\{[^}]*outline:\s*var\(--focus-ring\);/s,
    );
  });

  it("keeps natural-width composer controls available when an artifact pane narrows Chat", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.select-control\s*\{[^}]*max-width:\s*none;[^}]*grid-template-columns:\s*auto\s+minmax\(0,\s*1fr\);/s,
    );
  });

  it("aligns the measured card with the transcript column without hiding access", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.composer,\s*\.chat-surface > \.acp-permission-prompt\s*\{[^}]*width:\s*min\(\s*var\(--reading-measure\),[^}]*margin-inline-start:\s*max\(\s*var\(--chat-column-gutter\),\s*calc\(\(100% - var\(--chat-column-trailing\) - var\(--reading-measure\)\) \/ 2\)\s*\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer textarea\s*\{[^}]*min-height:\s*var\(--composer-input-min-block-size\);[^}]*padding:\s*var\(--space-3\)\s+var\(--space-3\)\s+var\(--space-1\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__shortcut\s*\{[^}]*display:\s*none;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__error,\s*\.composer__notice\s*\{[^}]*align-items:\s*center;[^}]*align-self:\s*start;/s,
    );
    expect(surfaceCss).not.toMatch(/\.access-mode\s*\{[^}]*display:\s*none;/s);
    expect(
      await readFile(resolve("src/styles/composer-surface.css"), "utf8"),
    ).not.toMatch(/transform:\s*translate/);
  });

  it("owns an accessible token usage progressbar with tokenized and forced-color styling", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(/^\.composer__context-usage\b/m);
    expect(surfaceCss).toMatch(
      /\.composer__context-usage::before\s*\{[^}]*background:\s*conic-gradient\(/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__input-area\s*\{[^}]*display:\s*flex;[^}]*flex-direction:\s*column;/s,
    );
    const contextRule = surfaceCss.match(/^\.composer__context\s*\{([^}]*)\}/m);
    expect(contextRule).not.toBeNull();
    const contextBody = contextRule?.[1] ?? "";
    expect(contextBody).not.toMatch(/position:/);
    expect(contextBody).not.toMatch(/\b(top|right|bottom|left):/);
    expect(contextBody).toMatch(/display:\s*flex;/);
    expect(contextBody).toMatch(/justify-content:\s*flex-end;/);
    expect(contextBody).toMatch(/align-items:\s*center;/);
    expect(contextBody).toMatch(/gap:\s*var\(--space-2\);/);
    expect(contextBody).toMatch(/margin:\s*0;/);
    expect(contextBody).toMatch(/grid-area:\s*meter;/);
    expect(contextBody).toMatch(/padding:\s*0;/);
    expect(contextBody).toMatch(/font-size:\s*var\(--font-size-xs\);/);
    expect(contextBody).toMatch(/color:\s*var\(--color-text-muted\);/);
    expect(contextBody).not.toMatch(/pointer-events:\s*none;/);
    expect(surfaceCss).toMatch(
      /\.project-scope\s*\{[^}]*height:\s*var\(--control-height-compact\);[^}]*gap:\s*var\(--space-1\);[^}]*border:\s*1px solid var\(--glass-border\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.project-scope__icon,\s*\.select-control__icon\s*\{[^}]*flex:\s*0\s+0\s+auto;[^}]*color:\s*var\(--color-text-muted\);/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__context-usage::before\s*\{[^}]*var\(--composer-context-usage\)[^}]*mask:\s*radial-gradient\(/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__context-usage:focus-visible\s*\{[^}]*outline:\s*var\(--focus-ring\);[^}]*outline-offset:\s*var\(--focus-ring-offset\);/s,
    );
    expect(
      await readFile(resolve("src/styles/status-surfaces.css"), "utf8"),
    ).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{\s*\.send-button\s*\{[^}]*border:\s*1px solid CanvasText;/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*?\.composer__context-usage::before\s*\{[^}]*border:[^}]*CanvasText;/s,
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

  it("keeps attached chips available without a disabled affordance row", async () => {
    const surfaceCss = await composerRules();

    expect(surfaceCss).toMatch(
      /\.composer__attachment-row\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s,
    );
    expect(surfaceCss).toMatch(
      /\.composer__attachments\s*\{[^}]*flex:\s*1\s+1\s+auto;[^}]*padding:\s*0;/s,
    );
    expect(surfaceCss).toMatch(
      /\.send-button\s*\{[^}]*width:\s*28px;[^}]*height:\s*28px;[^}]*border-radius:\s*var\(--radius-circle\);[^}]*background:\s*var\(--color-status-foreground\);/s,
    );
    expect(surfaceCss).not.toContain(".composer__affordances");
    expect(surfaceCss).not.toMatch(
      /@media \(max-height:\s*360px\)[\s\S]*?\.composer__attachment-row\s*\{[^}]*display:\s*none;/s,
    );
  });

  it("draws no composer focus border, as Codex, but keeps a forced-colors outline", async () => {
    const surfaceCss = await composerRules();
    // Codex shows focus by the caret alone (s2/f0113, s2/f0244; W9 B9).
    const defaultCss = surfaceCss.replace(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*$/,
      "",
    );
    expect(defaultCss).not.toMatch(/\.composer:focus-within\s*\{/);
    expect(surfaceCss).not.toMatch(
      /\.composer:focus-within\s*\{[^}]*0\s+0\s+0\s+var\(--focus-ring-offset\)/s,
    );
    expect(surfaceCss).toMatch(
      /@media \(forced-colors:\s*active\)\s*\{[\s\S]*?\.composer:focus-within\s*\{[^}]*border-color:\s*Highlight;[^}]*outline:\s*2px solid Highlight;[^}]*outline-offset:\s*-2px;[^}]*box-shadow:\s*none;/s,
    );
  });
});
