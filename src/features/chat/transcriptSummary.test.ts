import { describe, expect, it } from "vitest";

import {
  formatWorkedDuration,
  formatWorkingDuration,
  groupTranscriptActivities,
  summarizeActivityKinds,
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

  it("summarizes adjacent activity kinds in their observed order", () => {
    expect(
      summarizeActivityKinds([
        { kind: "edit", count: 1 },
        { kind: "execute", count: 2 },
      ]),
    ).toBe("Edited a file, ran commands");
    expect(summarizeActivityKinds([{ kind: "execute", count: 2 }])).toBe(
      "Ran commands",
    );
    expect(summarizeActivityKinds([{ kind: "read", count: 4 }])).toBe(
      "Explored 4 files",
    );
    expect(summarizeActivityKinds([])).toBeNull();
  });

  it("keeps edit and command work together and separates later activity groups", () => {
    const groups = groupTranscriptActivities([
      { kind: "edit", count: 1 },
      { kind: "execute", count: 2 },
      null,
      { kind: "execute", count: 1 },
      null,
      { kind: "read", count: 4 },
    ]);

    expect(groups.map((group) => summarizeActivityKinds(group))).toEqual([
      "Edited a file, ran commands",
      "Ran commands",
      "Explored 4 files",
    ]);
  });
});
