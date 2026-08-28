import { describe, expect, it } from "vitest";

import { resizeAgentChatSplit } from "./agentChatSplit";

describe("resizeAgentChatSplit", () => {
  it("keeps the dual-chat divider within the usable 30–70% range", () => {
    expect(
      resizeAgentChatSplit({ containerWidth: 1_000, pointerOffset: 120 }),
    ).toBe(30);
    expect(
      resizeAgentChatSplit({ containerWidth: 1_000, pointerOffset: 560 }),
    ).toBe(56);
    expect(
      resizeAgentChatSplit({ containerWidth: 1_000, pointerOffset: 880 }),
    ).toBe(70);
  });

  it("falls back to an even split when no container width is measurable", () => {
    expect(
      resizeAgentChatSplit({ containerWidth: 0, pointerOffset: 560 }),
    ).toBe(50);
  });
});
