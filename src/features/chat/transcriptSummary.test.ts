import { describe, expect, it } from "vitest";

import {
  formatWorkedDuration,
  formatWorkingDuration,
} from "./transcriptSummary";

describe("transcript summaries", () => {
  it("formats completed work duration without a wall clock", () => {
    expect(formatWorkedDuration(0, 192_000)).toBe("Worked for 3m 12s");
    expect(formatWorkedDuration(0, 45_000)).toBe("Worked for 45s");
    expect(formatWorkedDuration(0, 9_660_000)).toBe("Worked for 2h 41m");
    expect(formatWorkedDuration(1_000, null)).toBeNull();
    expect(formatWorkedDuration(2_000, 1_000)).toBeNull();
    expect(formatWorkingDuration(1_000, 49_011_000)).toBe(
      "Working for 13h 36m",
    );
    expect(formatWorkingDuration(1_000, 4_000)).toBe("Working for 3s");
  });
});
