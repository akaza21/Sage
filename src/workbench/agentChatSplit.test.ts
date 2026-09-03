import { describe, expect, it } from "vitest";

import {
  AGENT_CHAT_SPLIT_MIN_CONTAINER,
  canShowAgentChatSplit,
  resizeAgentChatSplit,
} from "./agentChatSplit";

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

describe("canShowAgentChatSplit", () => {
  it("requires enough measured center width for two usable chat panes", () => {
    expect(canShowAgentChatSplit(AGENT_CHAT_SPLIT_MIN_CONTAINER - 1)).toBe(
      false,
    );
    expect(canShowAgentChatSplit(AGENT_CHAT_SPLIT_MIN_CONTAINER)).toBe(true);
    expect(canShowAgentChatSplit(AGENT_CHAT_SPLIT_MIN_CONTAINER + 1)).toBe(
      true,
    );
  });

  it("fails closed when the center width is not measurable", () => {
    expect(canShowAgentChatSplit(0)).toBe(false);
    expect(canShowAgentChatSplit(Number.NaN)).toBe(false);
    expect(canShowAgentChatSplit(Number.POSITIVE_INFINITY)).toBe(false);
  });
});
