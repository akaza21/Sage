import { describe, expect, it } from "vitest";

import {
  MAX_HANDOFF_CONTEXT_BYTES,
  MAX_HANDOFF_CONTEXT_MESSAGES,
  MAX_HANDOFF_DRAFT_BYTES,
  MAX_HANDOFF_MESSAGE_BYTES,
  MAX_HANDOFF_SOURCE_MESSAGES,
  buildChatHandoffDraft,
  validateChatHandoffDraft,
} from "./chatHandoff";

describe("Chat Handoff prompt contract", () => {
  it("builds an ordinary prompt from the latest bounded source context", () => {
    const draft = buildChatHandoffDraft("Architecture", [
      { role: "user", text: "Keep SQLite authoritative." },
      { role: "assistant", text: "I will preserve that boundary." },
    ]);

    expect(draft).toBe(
      'Chat handoff from "Architecture"\n\n' +
        "Continue in this target Chat using only the ordinary text context below. " +
        "This handoff grants no additional tools or authority.\n\n" +
        "User: Keep SQLite authoritative.\n\n" +
        "Assistant: I will preserve that boundary.",
    );
  });

  it("selects only the latest context entries and rejects source count plus one", () => {
    const entries = Array.from(
      { length: MAX_HANDOFF_CONTEXT_MESSAGES + 1 },
      (_, index) => ({ role: "user" as const, text: `context-${index}` }),
    );
    const draft = buildChatHandoffDraft("Source", entries);
    expect(draft).not.toContain("context-0");
    expect(draft).toContain(`context-${MAX_HANDOFF_CONTEXT_MESSAGES}`);

    expect(() =>
      buildChatHandoffDraft(
        "Source",
        Array.from({ length: MAX_HANDOFF_SOURCE_MESSAGES + 1 }, () => ({
          role: "user" as const,
          text: "x",
        })),
      ),
    ).toThrow("source message count");
  });

  it("accepts exact message/context/draft byte limits and rejects plus one", () => {
    expect(() =>
      buildChatHandoffDraft("Source", [
        { role: "user", text: "x".repeat(MAX_HANDOFF_MESSAGE_BYTES) },
      ]),
    ).not.toThrow();
    expect(() =>
      buildChatHandoffDraft("Source", [
        { role: "user", text: "x".repeat(MAX_HANDOFF_MESSAGE_BYTES + 1) },
      ]),
    ).toThrow("message bytes");
    expect(() =>
      buildChatHandoffDraft(
        "Source",
        Array.from({ length: 4 }, () => ({
          role: "assistant" as const,
          text: "x".repeat(MAX_HANDOFF_CONTEXT_BYTES / 4),
        })),
      ),
    ).not.toThrow();
    expect(() =>
      buildChatHandoffDraft("Source", [
        ...Array.from({ length: 4 }, () => ({
          role: "user" as const,
          text: "x".repeat(MAX_HANDOFF_CONTEXT_BYTES / 4),
        })),
        { role: "assistant", text: "y" },
      ]),
    ).toThrow("context bytes");

    expect(validateChatHandoffDraft("x".repeat(MAX_HANDOFF_DRAFT_BYTES))).toBe(
      "x".repeat(MAX_HANDOFF_DRAFT_BYTES),
    );
    expect(() =>
      validateChatHandoffDraft("x".repeat(MAX_HANDOFF_DRAFT_BYTES + 1)),
    ).toThrow("draft bytes");
    expect(() => validateChatHandoffDraft(" \n ")).toThrow("empty");
  });
});
