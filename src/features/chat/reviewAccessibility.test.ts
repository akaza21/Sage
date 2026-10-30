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

it("reveals hover times for inherited and truly final virtualized Turns", () => {
  expect(read("src/features/chat/fork-chat.css")).toMatch(
    /\.chat-fork-turn\s*\{\s*min-width:\s*0;/,
  );
  const css = read("src/features/chat/chat-transcript-polish.css");
  expect(css).toContain(".chat-fork-turn:hover .chat-transcript-actions__time");
  expect(css).toMatch(
    /:has\(\s*> \.acp-chat-projection > \.acp-virtual-turns > \[data-last-turn\]:hover/,
  );
});

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

it("only preserves disabled model opacity for explicit read-only labels", () => {
  const css = read("src/styles/composer-model-picker.css");
  expect(css).toMatch(
    /\.composer__model-trigger\[data-readonly-label\]:disabled\s*\{\s*opacity: 1;/,
  );
  expect(css).not.toContain(".composer__model-trigger[title]:disabled");
});

it("places Agent find below header controls without moving other chat find bars", () => {
  const css = read("src/features/chat/chat-find.css");
  expect(css).toMatch(
    /\.chat-surface--agent > \.chat-find\s*\{[^}]*grid-area:\s*find;/,
  );
  expect(css).not.toMatch(/top:\s*-36px/);
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
  const background = channels(
    global.match(/--color-plane-raised:\s*#([\da-f]{6});/)![1],
  );
  const accent = channels(global.match(/--color-accent:\s*#([\da-f]{6});/)![1]);
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
    (luminance(channels(edge)) + 0.05) / (luminance(channels("2d2d2d")) + 0.05),
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
