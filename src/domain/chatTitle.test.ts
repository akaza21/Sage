import { describe, expect, it, vi } from "vitest";

import {
  MAX_CHAT_TITLE_BYTES,
  canonicalChatName,
  canonicalChatTitle,
  chatDisplayTitle,
} from "./chatTitle";

describe("Chat title contract", () => {
  it("trims the display title into the stored Chat projection", () => {
    expect(canonicalChatTitle("  Focus  ")).toBe("Chat: Focus");
    expect(canonicalChatTitle("\ufeff\u0085\u2003 Focus \u3000\ufeff")).toBe(
      "Chat: Focus",
    );
    expect(chatDisplayTitle("Chat: Focus")).toBe("Focus");
    expect(chatDisplayTitle("Legacy title")).toBe("Legacy title");
  });

  it("bounds the trimmed display title by UTF-8 bytes", () => {
    const exact = "é".repeat(MAX_CHAT_TITLE_BYTES / 2);
    expect(canonicalChatTitle(exact)).toBe(`Chat: ${exact}`);
    expect(() => canonicalChatTitle(`${exact}x`)).toThrow("128 UTF-8 bytes");
  });

  it("rejects raw overlength input before UTF-8 encoding", () => {
    const encode = vi.spyOn(TextEncoder.prototype, "encode");
    try {
      expect(() => canonicalChatName("x".repeat(129))).toThrow(
        "128 UTF-8 bytes",
      );
      expect(() => canonicalChatName("x".repeat(2 * 1024 * 1024))).toThrow(
        "128 UTF-8 bytes",
      );
      expect(encode).not.toHaveBeenCalled();
    } finally {
      encode.mockRestore();
    }
  });

  it("rejects empty and control-bearing display titles", () => {
    expect(() => canonicalChatTitle("\u{2003} \t")).toThrow("not be empty");
    expect(() => canonicalChatTitle("\ufeff")).toThrow("not be empty");
    expect(() => canonicalChatTitle("\u0085")).toThrow("not be empty");
    expect(() => canonicalChatTitle("line\nbreak")).toThrow(
      "control characters",
    );
    expect(() => canonicalChatTitle("Focus\u0085Later")).toThrow(
      "control characters",
    );
    for (const malformed of ["Focus\ud800", "Focus\udc00"]) {
      expect(() => canonicalChatTitle(malformed)).toThrow(
        "unpaired UTF-16 surrogate",
      );
    }
    expect(canonicalChatTitle("Focus \ud83d\ude80")).toBe("Chat: Focus 🚀");
  });
});
