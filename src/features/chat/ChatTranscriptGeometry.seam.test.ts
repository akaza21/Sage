import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it } from "vitest";

const sheets: HTMLStyleElement[] = [];

function compact(value: string) {
  return value.replace(/\s+/gu, "");
}

function declaration(path: string, selector: string, media?: string) {
  const sheet = document.createElement("style");
  sheet.textContent = readFileSync(path, "utf8");
  document.head.append(sheet);
  sheets.push(sheet);
  const rules = Array.from(sheet.sheet!.cssRules);
  const scope = media
    ? Array.from(
        (
          rules.find(
            (rule) =>
              rule instanceof CSSMediaRule && rule.conditionText === media,
          ) as CSSMediaRule
        ).cssRules,
      )
    : rules;
  const rule = scope.find(
    (rule) => rule instanceof CSSStyleRule && rule.selectorText === selector,
  ) as CSSStyleRule | undefined;
  expect(rule, selector).toBeDefined();
  return rule!.style;
}

afterEach(() => {
  for (const sheet of sheets.splice(0)) sheet.remove();
});

describe("CP-28 transcript style contracts", () => {
  it("does not recenter split composers over the shared transcript-column offset", () => {
    const split = readFileSync("src/styles/agent-chat-panes.css", "utf8");
    expect(split).not.toMatch(
      /\.agent-chat-panes--split \.composer\s*\{[^}]*margin-(?:left|right):\s*auto;/su,
    );
  });

  it("shares the Chat reading measure and pane-local gutters with the composer without a narrow cap", () => {
    const surface = declaration("src/styles/chat-surface.css", ".chat-surface");
    expect(surface.getPropertyValue("--reading-measure")).toBe("660px");
    expect(surface.getPropertyValue("--chat-column-gutter")).toBe(
      "var(--space-4)",
    );
    expect(surface.getPropertyValue("--chat-column-trailing")).toBe(
      "calc(var(--scrollbar-size) + var(--space-3))",
    );
    const composer = declaration(
      "src/styles/composer-surface.css",
      ".composer",
    );
    expect(compact(composer.width)).toBe(
      compact(
        "min(var(--reading-measure), calc(100% - var(--chat-column-gutter) - var(--chat-column-gutter) - var(--chat-column-trailing)))",
      ),
    );
    expect(compact(composer.marginInlineStart)).toBe(
      compact(
        "max(var(--chat-column-gutter), calc((100% - var(--chat-column-trailing) - var(--reading-measure)) / 2))",
      ),
    );
    const source = readFileSync("src/styles/composer-surface.css", "utf8");
    expect(source).not.toMatch(/466px/);
    expect(
      declaration(
        "src/features/chat/chat-transcript-polish.css",
        ".message-list__content",
      ).getPropertyValue("--reading-measure"),
    ).toBe("");
    expect(
      declaration(
        "src/features/chat/chat-transcript-polish.css",
        ".message-list__content .acp-turn__items",
      ).width,
    ).toBe("min(100%, var(--reading-measure))");
  });

  it("configures one native-marker gutter and hanging text inset for Chat lists", () => {
    const list = declaration(
      "src/features/chat/acp/chat-markdown.css",
      ".acp-markdown :is(ul, ol)",
    );
    expect(compact(list.paddingInlineStart)).toBe(
      compact(
        "max(var(--space-4), calc(var(--chat-list-marker-width) + 0.5ch))",
      ),
    );
    const item = declaration(
      "src/features/chat/acp/chat-markdown.css",
      ".acp-markdown li",
    );
    expect(item.paddingInlineStart).toBe("var(--space-2)");
    const marker = declaration(
      "src/features/chat/acp/chat-markdown.css",
      ".acp-markdown li::marker",
    );
    expect(marker.fontVariantNumeric).toBe("tabular-nums");
  });

  it("removes only Chat tables' leading cell inset and explicitly sets their typography", () => {
    const leading = declaration(
      "src/features/chat/acp/chat-markdown.css",
      ".acp-markdown .markdown-table :is(th, td):first-child",
    );
    expect(leading.paddingInlineStart).toBe("0px");
    const cells = declaration(
      "src/features/chat/acp/chat-markdown.css",
      ".acp-markdown .markdown-table :is(th, td)",
    );
    expect(cells.fontSize).toBe("var(--font-size-sm)");
    expect(cells.lineHeight).toBe("var(--line-height-relaxed)");
    const headers = declaration(
      "src/features/chat/acp/chat-markdown.css",
      ".acp-markdown .markdown-table th",
    );
    expect(headers.fontWeight).toBe("var(--font-weight-semibold)");
  });

  it("configures a filled borderless code panel and restores a forced-colors boundary", () => {
    const panel = declaration(
      "src/features/chat/acp/acp-chat.css",
      ".acp-code-block",
    );
    expect(panel.border).toBe("0px");
    expect(panel.background).toBe("var(--color-surface-hover)");
    expect(panel.borderRadius).toBe("var(--radius-xl)");
    const forced = declaration(
      "src/features/chat/acp/acp-chat.css",
      ".acp-code-block",
      "(forced-colors: active)",
    );
    expect(forced.border).toBe("1px solid canvastext");
  });

  it("configures a persistent Worked disclosure indicator without requiring hover", () => {
    const indicator = declaration(
      "src/features/chat/chat-transcript-polish.css",
      ".chat-transcript-work > summary::after",
    );
    expect(indicator.display).toBe("block");
    expect(indicator.getPropertyValue("flex-shrink")).toBe("0");
  });
});
