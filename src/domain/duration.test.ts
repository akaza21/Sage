import { describe, expect, it } from "vitest";
import { elapsedSecondsBetween, formatElapsedSeconds } from "./duration";

describe("formatElapsedSeconds", () => {
  it.each([
    [0, "0s"],
    [0.4, "0s"],
    [45, "45s"],
    [192, "3m 12s"],
    [300, "5m"],
    [5_305, "1h 28m 25s"],
    [55_182, "15h 19m 42s"],
    [9_660, "2h 41m"],
    [284_415, "3d 7h 15s"],
  ])("formats %s seconds as %s", (seconds, label) => {
    expect(formatElapsedSeconds(seconds)).toBe(label);
  });
});

describe("elapsedSecondsBetween", () => {
  it("measures forward intervals and rejects open, reversed or invalid ones", () => {
    expect(elapsedSecondsBetween(1_000, 4_500)).toBe(3.5);
    expect(elapsedSecondsBetween(1_000, null)).toBeNull();
    expect(elapsedSecondsBetween(2_000, 1_000)).toBeNull();
    expect(elapsedSecondsBetween(Number.NaN, 1_000)).toBeNull();
  });
});
