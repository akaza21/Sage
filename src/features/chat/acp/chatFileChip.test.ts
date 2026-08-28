import { describe, expect, it } from "vitest";

import {
  CHAT_FILE_CHIP_NAME_CAP,
  chatFileChipBasename,
  chatFileChipCount,
} from "./chatFileChip";

describe("chat file chip labels", () => {
  it("uses a bounded basename and rejects empty or control paths", () => {
    expect(chatFileChipBasename("src/main.ts")).toBe("main.ts");
    expect(chatFileChipBasename("src\\engine.py")).toBe("engine.py");
    expect(chatFileChipBasename("  /docs/readme.md  ")).toBe("readme.md");
    expect(
      chatFileChipBasename(`${"n".repeat(CHAT_FILE_CHIP_NAME_CAP)}.ts`),
    ).toBe(`${"n".repeat(CHAT_FILE_CHIP_NAME_CAP - 1)}…`);
    expect(chatFileChipBasename("")).toBeNull();
    expect(chatFileChipBasename("   ")).toBeNull();
    expect(chatFileChipBasename("bad\nfile.ts")).toBeNull();
    expect(chatFileChipBasename("///")).toBeNull();
  });

  it("formats bounded +N/−N counts and omits unknown values", () => {
    expect(chatFileChipCount("+", 12)).toBe("+12");
    expect(chatFileChipCount("-", 0)).toBe("-0");
    expect(chatFileChipCount("+", 1000)).toBe("+999+");
    expect(chatFileChipCount("-", null)).toBeNull();
  });
});
