import { describe, expect, it } from "vitest";

import {
  TITLE_BAR_LABEL_CAP,
  titleBarFocusedChatId,
  titleBarSessionTitle,
} from "./titleBarCenter";

describe("title bar center labels", () => {
  it("shows a bounded session title and rejects empty or control text", () => {
    expect(titleBarSessionTitle("Chat: Onboarding")).toBe("Onboarding");
    expect(titleBarSessionTitle("Review PR")).toBe("Review PR");
    expect(
      titleBarSessionTitle(`Chat: ${"n".repeat(TITLE_BAR_LABEL_CAP + 2)}`),
    ).toBe(`${"n".repeat(TITLE_BAR_LABEL_CAP - 1)}…`);
    expect(titleBarSessionTitle("")).toBeNull();
    expect(titleBarSessionTitle("   ")).toBeNull();
    expect(titleBarSessionTitle("Chat: bad\ntitle")).toBeNull();
    expect(titleBarSessionTitle(null)).toBeNull();
  });

  it("uses the active chat or the first remaining chat", () => {
    expect(titleBarFocusedChatId(["chat-a", "chat-b"], "chat-b")).toBe(
      "chat-b",
    );
    expect(titleBarFocusedChatId(["chat-a", "chat-b"], "file:app")).toBe(
      "chat-a",
    );
    expect(titleBarFocusedChatId([], "file:app")).toBeNull();
  });
});
