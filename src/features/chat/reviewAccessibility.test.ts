import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

const read = (path: string) => readFileSync(path, "utf8");
const channels = (hex: string) =>
  (hex.match(/../g) ?? []).map((channel) => parseInt(channel, 16));
const luminance = (rgb: readonly number[]) =>
  rgb.reduce((total, channel, index) => {
    const value = channel / 255;
    return (
      total +
      (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4) *
        [0.2126, 0.7152, 0.0722][index]
    );
  }, 0);
/** A token's six-digit hex, following plain var() aliases. */
const rootHex = (css: string, token: string): string => {
  const value = css.match(new RegExp(`--${token}:\\s*([^;]+);`))![1].trim();
  const alias = value.match(/^var\(--([\w-]+)\)$/);
  return alias ? rootHex(css, alias[1]) : value.match(/^#([\da-f]{6})$/)![1];
};

it("keeps sent annotation chip outlines in forced colours", () => {
  const css = read("src/features/chat/transcript-interactions.css");
  expect(css).toMatch(
    /\.annotation-chip--sent \.annotation-chip__trigger\s*\{[^}]*outline:\s*1px solid/,
  );
  expect(css).toContain("outline: 1px solid var(--chat-annotation-sent-edge)");
  expect(
    read("src/styles/global.css").split("@media (forced-colors: active)")[1],
  ).toMatch(/--chat-annotation-sent-edge:\s*CanvasText;/);
});

it("keeps fork keyboard focus visible and maps forced-colour hover to Highlight", () => {
  const fork = read("src/features/chat/fork-chat.css");
  const global = read("src/styles/global.css");
  expect(fork).not.toMatch(/button:focus-visible\s*\{\s*outline: none/);
  expect(global.split("@media (forced-colors: active)")[1]).toMatch(
    /--chat-dialog-option-hover:\s*Highlight;/,
  );
  expect(global).toMatch(
    /\):focus-visible\s*\{\s*outline: var\(--focus-ring\);/,
  );
  const background = channels(rootHex(global, "color-plane-raised"));
  const accentStep = global.match(/--color-accent:\s*var\((--[\w-]+)\);/)![1];
  const accent = channels(
    global.match(new RegExp(`${accentStep}:\\s*#([\\da-f]{6});`))![1],
  );
  const opacity =
    Number(
      global.match(
        /--highlight-focus-edge: color-mix\(\s*in srgb,\s*var\(--color-accent\) (\d+)%/,
      )![1],
    ) / 100;
  const ring = accent.map(
    (value, index) => value * opacity + background[index] * (1 - opacity),
  );
  expect(
    (luminance(ring) + 0.05) / (luminance(background) + 0.05),
  ).toBeGreaterThanOrEqual(3);
});

it("keeps the close-side-chat checkbox visible in both colour modes at 3:1", () => {
  const global = read("src/styles/global.css");
  const edge = global.match(
    /--chat-dialog-checkbox-edge:\s*#([\da-f]{6});/,
  )![1];
  expect(
    (luminance(channels(edge)) + 0.05) /
      (luminance(channels(rootHex(global, "color-panel-raised"))) + 0.05),
  ).toBeGreaterThanOrEqual(3);
  const forced = global.split("@media (forced-colors: active)")[1];
  expect(forced).toMatch(/--chat-dialog-checkbox-edge:\s*CanvasText;/);
  const css = read("src/features/chat/transcript-interactions.css");
  expect(forced).toMatch(/--color-status-foreground:\s*CanvasText;/);
  expect(css).toMatch(
    /\.close-side-chat-dialog input\[type="checkbox"\]\s*\{[^}]*forced-color-adjust:\s*none;/,
  );
  expect(css).toMatch(
    /\.close-side-chat-dialog input:checked\s*\{[^}]*background:\s*currentColor content-box;/,
  );
});
