import { describe, expect, it } from "vitest";

import { truncateUtf8, utf8ByteLength } from "./text";

describe("shared UTF-8 text helpers", () => {
  it("measures ASCII, multibyte, and surrogate-pair text", () => {
    expect(utf8ByteLength("abc")).toBe(3);
    expect(utf8ByteLength("é")).toBe(2);
    expect(utf8ByteLength("🚀")).toBe(4);
  });

  it("truncates a byte prefix without a replacement character", () => {
    expect(truncateUtf8("a🚀b", 4)).toBe("a");
    expect(truncateUtf8("a🚀b", 5)).toBe("a🚀");
    expect(utf8ByteLength(truncateUtf8("🚀".repeat(1024), 2_048))).toBe(2_048);
  });
});
