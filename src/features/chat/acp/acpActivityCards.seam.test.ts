import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("ACP activity-card stylesheet seam", () => {
  it("owns the shared card grammar without leaving duplicate selectors", async () => {
    const [projection, cards, legacy, chat] = await Promise.all([
      readFile(resolve("src/features/chat/acp/AcpChatProjection.tsx"), "utf8"),
      readFile(resolve("src/features/chat/acp/acp-activity-cards.css"), "utf8"),
      readFile(
        resolve("src/features/chat/acp/acp-chat-activities.css"),
        "utf8",
      ),
      readFile(resolve("src/features/chat/acp/acp-chat.css"), "utf8"),
    ]);

    expect(projection).toMatch(/import "\.\/acp-activity-cards\.css";/);
    expect(cards).toMatch(/^\.acp-activity-card\s*\{/m);
    expect(cards).toMatch(/data-activity-state="inProgress"/);
    expect(cards).toMatch(
      /\.acp-plan__working\s*\{[^}]*animation:\s*sage-working-pulse/s,
    );
    expect(cards).toMatch(/\.acp-plan__activity\s*\{/);
    expect(cards).toMatch(/@media \(prefers-reduced-motion: reduce\)/);
    expect(cards).toMatch(/@media \(forced-colors: active\)/);
    expect(legacy).not.toMatch(/^\.acp-tool-card\b/m);
    expect(legacy).not.toMatch(/^\.acp-plan\b/m);
    expect(legacy).not.toMatch(/^\.acp-thought\b/m);
    expect(chat).not.toMatch(/^\.acp-usage\b/m);
  });
});
