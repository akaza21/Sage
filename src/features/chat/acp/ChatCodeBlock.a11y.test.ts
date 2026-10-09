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
  const background = resolve(rule.match(/background:\s*([^;]+);/u)![1]);
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
