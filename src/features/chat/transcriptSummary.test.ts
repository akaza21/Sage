import { describe, expect, it } from "vitest";

import { formatDuration } from "./transcriptSummary";

describe("transcript summaries", () => {
  it("formats completed work duration without a wall clock", () => {
    expect(formatDuration("Worked", 0, 192_000)).toBe("Worked for 3m 12s");
    expect(formatDuration("Worked", 0, 45_000)).toBe("Worked for 45s");
    expect(formatDuration("Worked", 0, 9_660_000)).toBe("Worked for 2h 41m");
    expect(formatDuration("Worked", 0, 5_305_000)).toBe(
      "Worked for 1h 28m 25s",
    );
    expect(formatDuration("Worked", 1_000, null)).toBeNull();
    expect(formatDuration("Worked", 2_000, 1_000)).toBeNull();
    expect(formatDuration("Working", 1_000, 49_011_000)).toBe(
      "Working for 13h 36m 50s",
    );
    expect(formatDuration("Working", 1_000, 4_000)).toBe("Working for 3s");
  });
});
