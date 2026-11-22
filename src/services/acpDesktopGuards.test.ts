import { expect, it } from "vitest";
import { safeMessage, safeText } from "./acpDesktopGuards";

it.each([
  ["abc", 9, "abc"],
  ["abc", 2, null],
  ["€", 3, "€"],
  ["€", 2, null],
  ["😀", 4, "😀"],
  ["😀", 3, null],
  ["\ud800", 3, "\ud800"],
  ["\ud800", 2, null],
  ["", 0, ""],
  [42, 1024, null],
] as const)("validates %j within %i UTF-8 bytes", (input, limit, expected) => {
  expect(safeText(input, limit)).toBe(expected);
});

it("keeps messages nonempty and admits exactly 8192 UTF-8 bytes", () => {
  // 2730 three-byte characters plus two ASCII bytes.
  const message = "€".repeat(2730) + "ab";
  expect(safeMessage(message)).toBe(message);
  expect(safeMessage(message + "c")).toBeNull();
  expect(safeMessage("")).toBeNull();
  expect(safeMessage(undefined)).toBeNull();
});
