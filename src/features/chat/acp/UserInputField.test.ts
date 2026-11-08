import { describe, expect, it } from "vitest";

import type { UserInputQuestion } from "../../../domain/acp";
import { itemsOk, parseAnswerNumber, textState } from "./UserInputField";

const question = (extra: Partial<UserInputQuestion>): UserInputQuestion => ({
  id: "q",
  header: "Q",
  question: "",
  allowsOther: false,
  secret: false,
  kind: "text",
  multiSelect: false,
  optional: false,
  ...extra,
});

describe("UserInputField answer checks", () => {
  it("reads numbers with the native grammar", () => {
    // Same cases as the native `numbers_follow_one_plain_decimal_grammar`.
    const cases: [string, bigint | null, number | null][] = [
      ["1.0", null, 1],
      ["1e3", null, 1000],
      ["0x10", null, null],
      [" 1", null, null],
      ["", null, null],
      ["-0", 0n, -0],
      ["+5", 5n, 5],
      [".5", null, 0.5],
      ["1.", null, 1],
      [".", null, null],
      ["1e", null, null],
      ["inf", null, null],
      ["NaN", null, null],
      ["Infinity", null, null],
      ["1e999", null, null],
      ["9223372036854775807", 2n ** 63n - 1n, 2 ** 63],
      ["9223372036854775808", null, 2 ** 63],
    ];
    for (const [text, integer, number] of cases) {
      expect(parseAnswerNumber("integer", text), text).toBe(integer);
      expect(parseAnswerNumber("number", text), text).toBe(number);
    }
  });

  it("checks text length in characters, number ranges and option counts", () => {
    const name = question({ limits: { minLength: 2, maxLength: 3 } });
    expect(textState(name, "é漢")).toBe("ok");
    expect(textState(name, "a")).toBe("badLength");
    expect(textState(name, "abcd")).toBe("badLength");
    const count = question({
      kind: "integer",
      limits: { minimum: -2, maximum: 5 },
    });
    for (const ok of ["-2", "5", "+5", " 0 "])
      expect(textState(count, ok)).toBe("ok");
    expect(textState(count, "6")).toBe("outOfRange");
    expect(textState(count, "-3")).toBe("outOfRange");
    expect(textState(count, "1.0")).toBe("notNumber");
    const ratio = question({
      kind: "number",
      limits: { minimum: 0.5, maximum: 1 },
    });
    expect(textState(ratio, "1.0")).toBe("ok");
    expect(textState(ratio, "5e-1")).toBe("ok");
    expect(textState(ratio, "0.4")).toBe("outOfRange");
    expect(textState(ratio, "0x1")).toBe("notNumber");
    const tags = question({
      multiSelect: true,
      limits: { minItems: 2, maxItems: 2 },
    });
    const picked = (picked: number[]) => ({
      picked,
      other: false,
      text: "empty" as const,
    });
    expect(itemsOk(tags, picked([0, 1]))).toBe(true);
    expect(itemsOk(tags, picked([0]))).toBe(false);
    expect(itemsOk(tags, picked([0, 1, 2]))).toBe(false);
  });
});
