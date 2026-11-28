import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

it("keeps shared syntax tokens readable against the code-card background token", async () => {
  const css = await readFile("src/styles/global.css", "utf8");
  const card = await readFile("src/features/chat/acp/acp-chat.css", "utf8");
  const rule = card.match(/\.acp-code-block \{([^}]+)\}/u)![1];
  const root = css.match(/:root \{([\s\S]*?)\n\}/u)![1];
  const tokens = new Map(
    [...root.matchAll(/(--[\w-]+):\s*([^;]+);/gu)].map((match) => [
      match[1],
      match[2],
    ]),
  );
  const resolve = (value: string): string => {
    const reference = /^var\((--[\w-]+)\)$/u.exec(value);
    return reference
      ? resolve(tokens.get(reference[1])!)
      : value
          .replace(/^#/u, "")
          .replace(/^([a-f0-9])([a-f0-9])([a-f0-9])$/u, "$1$1$2$2$3$3");
  };
  // A translucent fill is composited over the chat card it is drawn on.
  const over = (fill: string, card: string) => {
    const tint = /^rgb\((\d+) (\d+) (\d+) \/ ([\d.]+)%\)$/u.exec(fill);
    if (!tint) return fill;
    const alpha = Number(tint[4]) / 100;
    return [1, 2, 3]
      .map((i) =>
        Math.round(
          Number(tint[i]) * alpha +
            parseInt(card.slice((i - 1) * 2, i * 2), 16) * (1 - alpha),
        )
          .toString(16)
          .padStart(2, "0"),
      )
      .join("");
  };
  const background = over(
    resolve(rule.match(/background:\s*([^;]+);/u)![1]),
    resolve("var(--agent-shell-center)"),
  );
  const luminance = (hex: string) => {
    const channels = hex
      .match(/../gu)!
      .map((channel) => parseInt(channel, 16) / 255)
      .map((channel) =>
        channel <= 0.04045
          ? channel / 12.92
          : ((channel + 0.055) / 1.055) ** 2.4,
      );
    return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  };
  const colors = [...tokens].filter(([name]) =>
    name.startsWith("--doc-syntax-"),
  );
  expect(colors).toHaveLength(14);
  for (const color of colors)
    expect(
      (luminance(resolve(color[1])) + 0.05) / (luminance(background) + 0.05),
    ).toBeGreaterThanOrEqual(4.5);
});
