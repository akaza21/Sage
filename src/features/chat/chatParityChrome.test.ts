import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";

const css = (path: string) =>
  readFile(new URL(path, import.meta.url), "utf8").then((text) =>
    text.replace(/\/\*[\s\S]*?\*\//g, ""),
  );

function rule(text: string, selector: string) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`(?:^|})\\s*${escaped}\\s*{([^}]*)}`).exec(text);
  return match?.[1] ?? null;
}

describe("Codex chrome parity (w9 group B)", () => {
  it("draws no focus border on the composer (B9, s2/f0113)", async () => {
    const text = await css("../../styles/composer-surface.css");
    const [ordinary, forced] = text.split("@media (forced-colors: active)");
    expect(ordinary).not.toMatch(/\.composer:focus-within\s*{/);
    expect(forced).toMatch(/\.chat-dock:focus-within\s*{/);
  });

  it("sizes selection popover labels at 13 px with 7 px padding (B5, s2/f0093)", async () => {
    const text = await css("./transcript-interactions.css");
    const button = rule(text, ".chat-selection-popover button");
    expect(button).toMatch(/padding:\s*0 7px;/);
    expect(button).toMatch(/font-size:\s*13px;/);
  });

  it("sizes the comment field from the column, never the viewport (B1, s2/f0095)", async () => {
    const text = await css("./transcript-interactions.css");
    expect(rule(text, ".annotation-comment")).not.toMatch(/width:/);
    expect(text).not.toMatch(/100vw - 16px/);
  });

  it("outlines the badge numeral (B6, s2/f0113)", async () => {
    const text = await css("./transcript-interactions.css");
    const badge = rule(text, ".transcript-annotation-badge");
    expect(badge).toMatch(/font-weight:\s*600;/);
    expect(badge).toMatch(/box-shadow:/);
  });

  it("dims in-chat dialogs by 12% and tightens the close dialog (B4, B7, s2/f0352, s2/f0055)", async () => {
    const [dialogs, interactions, fork, global] = await Promise.all([
      css("../../styles/shared-dialogs.css"),
      css("./transcript-interactions.css"),
      css("./fork-chat.css"),
      css("../../styles/global.css"),
    ]);
    expect(rule(dialogs, ".dialog--scrim::backdrop")).toMatch(
      /var\(--chat-annotation-scrim\)/,
    );
    expect(global).toMatch(/--chat-annotation-scrim:\s*rgb\(0 0 0 \/ 12%\);/);
    expect(global).toMatch(/--chat-dialog-cancel-fill:\s*#353535;/);
    expect(
      rule(interactions, '.close-side-chat-dialog input[type="checkbox"]'),
      // 14 px, the small icon size token (an off-scale literal fails the seam).
    ).toMatch(
      /width:\s*var\(--icon-size-sm\);[\s\S]*height:\s*var\(--icon-size-sm\);/,
    );
    expect(global).toMatch(/--icon-size-sm:\s*14px;/);
    expect(
      rule(
        interactions,
        ".close-side-chat-dialog .dirty-dialog__actions .button",
      ),
    ).toMatch(/var\(--chat-dialog-cancel-fill\)/);
    expect(rule(interactions, ".close-side-chat-dialog")).toMatch(
      /font-size:\s*13px;/,
    );
    expect(fork).toMatch(
      /\.answer-more-menu__item,\s*\.fork-chat-dialog button\s*\{[^}]*color:\s*var\(--color-status-foreground\);/s,
    );
  });
});
