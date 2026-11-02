import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const css = (file: string) => readFileSync(resolve("src/styles", file), "utf8");

function hexRgb(value: string) {
  return [1, 3, 5].map((offset) =>
    parseInt(value.slice(offset, offset + 2), 16),
  );
}

function composite(foreground: number[], background: number[], alpha: number) {
  return foreground.map(
    (channel, index) => channel * alpha + background[index] * (1 - alpha),
  );
}

function contrast(foreground: number[], background: number[]) {
  const luminance = (channels: number[]) => {
    const linear = channels.map((channel) => {
      const scaled = channel / 255;
      return scaled <= 0.04045
        ? scaled / 12.92
        : ((scaled + 0.055) / 1.055) ** 2.4;
    });
    return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
  };
  const a = luminance(foreground);
  const b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

function rgbaToken(theme: string, name: string) {
  const match = new RegExp(
    `${name}:\\s*rgb\\((\\d+)\\s+(\\d+)\\s+(\\d+)\\s*\\/\\s*([\\d.]+)\\)`,
  ).exec(theme);
  if (!match) throw new Error(`Missing ${name}`);
  return { rgb: match.slice(1, 4).map(Number), alpha: Number(match[4]) };
}

function hexToken(theme: string, name: string) {
  const match = new RegExp(`${name}:\\s*(#[0-9a-f]{6});`, "i").exec(theme);
  if (!match) throw new Error(`Missing ${name}`);
  return hexRgb(match[1]);
}

describe("glass grey surfaces", () => {
  it("provides shared translucent glass roles and a solid accessibility fallback", () => {
    const theme = css("global.css");
    expect(theme).toMatch(/--glass-fill:\s*rgb\([^;]+\/[^;]+\);/);
    expect(theme).toMatch(/--glass-border:\s*rgb\([^;]+\/[^;]+\);/);
    expect(theme).toMatch(/--glass-highlight:\s*inset 0 1px 0/);
    expect(theme).toMatch(/--glass-blur:\s*blur\(/);
    expect(theme).toMatch(/@supports not \(backdrop-filter: blur\(1px\)\)/);
    expect(theme).toMatch(/@media \(prefers-reduced-transparency: reduce\)/);
  });

  it("keeps search glass while the Codex composer uses its opaque measured fill", () => {
    expect(css("project-navigation-surface.css")).toMatch(
      /\.project-action\s*\{[^}]*var\(--glass-fill\)/s,
    );
    expect(css("shared-dialogs.css")).toMatch(
      /\.chat-search-dialog\s*\{[^}]*background:\s*var\(--search-dialog-fill\);[^}]*backdrop-filter:\s*var\(--glass-blur\);/s,
    );
    expect(css("chat-navigation-surface.css")).toMatch(
      /\.project-chat-row\s*\{[^}]*var\(--glass-fill\)/s,
    );
    expect(css("composer-surface.css")).toMatch(
      /\.composer\s*\{[^}]*background:\s*var\(--composer-fill\);/s,
    );
    expect(css("composer-surface.css")).toMatch(
      /\.composer,\s*\.chat-surface > \.acp-permission-prompt\s*\{[^}]*border:\s*var\(--border-width\) solid var\(--composer-border\);/s,
    );
    expect(css("global.css")).toMatch(/--composer-border:\s*#383838;/);
    expect(css("global.css")).toMatch(/--composer-fill:\s*#363636;/);
    expect(css("composer-surface.css")).toMatch(
      /\.select-control--chip\s*\{[^}]*var\(--glass-fill\)/s,
    );
    expect(css("composer-surface.css")).toMatch(
      /\.project-scope\s*\{[^}]*var\(--glass-fill\)/s,
    );
  });

  it("keeps the title bar black and gives search dialog controls tokenized glass", () => {
    const theme = css("global.css");
    const chrome = css("workbench-chrome.css");
    const dialog = css("shared-dialogs.css");
    expect(theme).toMatch(/--title-bar-fill:\s*#[0-9a-f]+;/i);
    expect(chrome).toMatch(
      /\.title-bar\s*\{[^}]*background:\s*var\(--title-bar-fill\);/s,
    );
    expect(chrome).not.toMatch(
      /\.title-bar(?:__search|__chat-search--pill)?\s*\{[^}]*var\(--glass-fill\)/s,
    );
    expect(chrome).toMatch(
      /\.title-bar__chat-search--pill:hover:not\(:disabled\)\s*\{[^}]*background:\s*var\(--title-bar-control-hover\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__query input,[\s\S]*?background:\s*var\(--search-dialog-control-fill\);/,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__actions \.button\s*\{[^}]*background:\s*var\(--search-dialog-control-fill\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__actions \.button--primary:hover:not\(:disabled\)\s*\{[^}]*background:\s*var\(--search-dialog-button-hover\);/s,
    );
    expect(theme).toMatch(/--search-dialog-fill:\s*rgb\([^;]+\/[^;]+\);/);
    expect(/--search-dialog-control-fill:\s*([^;]+);/.exec(theme)?.[1]).toMatch(
      /^#[0-9a-f]{6}$/i,
    );
    expect(
      /--search-dialog-control-hover:\s*([^;]+);/.exec(theme)?.[1],
    ).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("keeps a subtle glass tint while obscuring transcript text without blur", () => {
    const theme = css("global.css");
    const dialog = css("shared-dialogs.css");
    const dialogAlpha = Number(
      /--search-dialog-fill:\s*rgb\([^/]+\/\s*([\d.]+)\)/.exec(theme)?.[1],
    );
    const scrimAlpha = Number(
      /--search-dialog-scrim:\s*rgb\([^/]+\/\s*([\d.]+)\)/.exec(theme)?.[1],
    );

    expect(dialogAlpha).toBeGreaterThanOrEqual(0.85);
    expect(dialogAlpha).toBeLessThanOrEqual(0.92);
    expect(scrimAlpha).toBeGreaterThanOrEqual(0);
    expect(scrimAlpha).toBeLessThanOrEqual(0.25);
    expect((1 - dialogAlpha) * (1 - scrimAlpha)).toBeLessThanOrEqual(0.12);
    expect(dialog).toMatch(
      /\.chat-search-dialog::backdrop\s*\{[^}]*background:\s*var\(--search-dialog-scrim\);/s,
    );
  });

  it("keeps every search dialog text role AA readable over a white underlay", () => {
    const theme = css("global.css");
    const dialog = css("shared-dialogs.css");
    const white = [255, 255, 255];
    const scrim = rgbaToken(theme, "--search-dialog-scrim");
    const fill = rgbaToken(theme, "--search-dialog-fill");
    const control = hexToken(theme, "--search-dialog-control-fill");
    const controlHover = hexToken(theme, "--search-dialog-control-hover");
    const surface = composite(
      fill.rgb,
      composite(scrim.rgb, white, scrim.alpha),
      fill.alpha,
    );
    const field = composite(control, surface, 1);
    const hoveredField = composite(controlHover, surface, 1);
    const primary = hexToken(
      theme,
      theme.includes("--search-dialog-text-primary:")
        ? "--search-dialog-text-primary"
        : "--color-text-primary",
    );
    const secondary = hexToken(theme, "--search-dialog-text");
    const action = hexToken(theme, "--search-dialog-button-fill");
    const actionHover = hexToken(theme, "--search-dialog-button-hover");

    for (const [text, background] of [
      [primary, surface],
      [secondary, surface],
      [primary, field],
      [primary, hoveredField],
      [primary, action],
      [primary, actionHover],
    ]) {
      expect(contrast(text, background)).toBeGreaterThanOrEqual(4.5);
    }
    expect(dialog).toMatch(
      /\.chat-search-dialog\s*\{[^}]*color:\s*var\(--search-dialog-text-primary\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-result\s*\{[^}]*color:\s*var\(--search-dialog-text-primary\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__form > p\s*\{[^}]*color:\s*var\(--search-dialog-text\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__query\s*\{[^}]*color:\s*var\(--search-dialog-text\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__actions \.button\s*\{[^}]*color:\s*var\(--search-dialog-text-primary\);/s,
    );
    expect(dialog).toMatch(
      /\.chat-search-dialog__query input,[\s\S]*?background:\s*var\(--search-dialog-control-fill\);/,
    );
  });

  it("keeps search errors readable over a white underlay", () => {
    const theme = css("global.css");
    const dialog = css("shared-dialogs.css");
    const scrim = rgbaToken(theme, "--search-dialog-scrim");
    const fill = rgbaToken(theme, "--search-dialog-fill");
    const surface = composite(
      fill.rgb,
      composite(scrim.rgb, [255, 255, 255], scrim.alpha),
      fill.alpha,
    );
    const errorBackground = theme.includes("--search-dialog-error-fill:")
      ? hexToken(theme, "--search-dialog-error-fill")
      : surface;

    expect(
      contrast(hexToken(theme, "--color-danger"), errorBackground),
    ).toBeGreaterThanOrEqual(4.5);
    expect(dialog).toMatch(
      /\.chat-search-dialog \.composer__error\s*\{[^}]*background:\s*var\(--search-dialog-error-fill\);/s,
    );
    expect(css("status-surfaces.css")).toMatch(
      /\.composer__error\s*\{[^}]*color:\s*var\(--color-danger\);/s,
    );
  });

  it("explicitly styles both empty and more-results text", () => {
    expect(css("shared-dialogs.css")).toMatch(
      /(?:^|\})\s*\.chat-search-dialog__more,\s*\.chat-search-dialog__empty\s*\{[^}]*color:\s*var\(--search-dialog-text\);/s,
    );
  });
});
