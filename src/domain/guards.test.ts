import { describe, expect, it } from "vitest";

import {
  MAX_DECIMAL_U64,
  boundedIdentifier,
  decimalU64Counter,
  isRecord,
  isShortId,
  safeOrdinal,
} from "./guards";

describe("shared domain guards", () => {
  it("accepts plain records, including arrays only when a caller permits them", () => {
    expect(isRecord({ value: 1 })).toBe(true);
    expect(isRecord([])).toBe(true);
    expect(isRecord(null)).toBe(false);
    expect(isRecord("record")).toBe(false);
  });

  it("bounds identifiers by UTF-16 and UTF-8 lengths", () => {
    expect(boundedIdentifier("x".repeat(512))).toBe("x".repeat(512));
    expect(boundedIdentifier("x".repeat(513))).toBeNull();
    expect(boundedIdentifier("é".repeat(256))).toBe("é".repeat(256));
    expect(boundedIdentifier("é".repeat(257))).toBeNull();
    expect(boundedIdentifier("ok\0no")).toBeNull();
  });

  it("accepts decimal u64 counters exactly and rejects every boundary plus one", () => {
    expect(decimalU64Counter("0", { allowZero: true })).toBe("0");
    expect(decimalU64Counter("0", { allowZero: false })).toBeNull();
    expect(decimalU64Counter(MAX_DECIMAL_U64, { allowZero: false })).toBe(
      MAX_DECIMAL_U64,
    );
    expect(
      decimalU64Counter("9223372036854775808", { allowZero: false }),
    ).toBeNull();
    expect(decimalU64Counter("01", { allowZero: true })).toBeNull();
  });
});

describe("shared short-id and ordinal guards", () => {
  it("admits a non-empty NUL-free string of at most 512 units", () => {
    expect(isShortId("chat-1")).toBe(true);
    expect(isShortId("a".repeat(512))).toBe(true);
    expect(isShortId("a".repeat(513))).toBe(false);
    expect(isShortId("")).toBe(false);
    expect(isShortId("a\0b")).toBe(false);
    expect(isShortId(7)).toBe(false);
  });

  it("returns non-negative safe integers and null otherwise", () => {
    expect(safeOrdinal(0)).toBe(0);
    expect(safeOrdinal(Number.MAX_SAFE_INTEGER)).toBe(Number.MAX_SAFE_INTEGER);
    expect(safeOrdinal(Number.MAX_SAFE_INTEGER + 1)).toBeNull();
    expect(safeOrdinal(-1)).toBeNull();
    expect(safeOrdinal(1.5)).toBeNull();
    expect(safeOrdinal("1")).toBeNull();
  });
});
