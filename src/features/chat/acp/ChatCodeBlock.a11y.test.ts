import { readFile } from "node:fs/promises";
import { expect, it } from "vitest";

it("keeps the chat-scoped syntax palette readable without changing Editor tokens", async () => {
  const css = await readFile("src/features/chat/acp/acp-chat.css", "utf8");
  const rule = css.match(/\.acp-code-block \{([^}]+)\}/u)![1];
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
  const colors = [...rule.matchAll(/--doc-syntax-[a-z-]+: #([a-f0-9]{6})/gu)];
  expect(colors).toHaveLength(14);
  for (const color of colors)
    expect(
      (luminance(color[1]) + 0.05) / (luminance("141414") + 0.05),
    ).toBeGreaterThanOrEqual(4.5);
});
