import { describe, expect, it } from "vitest";

import {
  countProblemKinds,
  problemSeverityLabel,
  problemSummaryTitle,
} from "./problemCard";

describe("problemSeverityLabel", () => {
  it("maps diagnostics onto Devin review-card labels", () => {
    expect(problemSeverityLabel("error")).toBe("Bug");
    expect(problemSeverityLabel("warning")).toBe("Flag");
    expect(problemSeverityLabel("info")).toBe("Informational");
  });
});

describe("problemSummaryTitle", () => {
  it("prefers bugs, then flags, then informational", () => {
    expect(problemSummaryTitle({ bugs: 1, flags: 3, infos: 2 })).toBe("1 Bug");
    expect(problemSummaryTitle({ bugs: 2, flags: 0, infos: 0 })).toBe("2 Bugs");
    expect(problemSummaryTitle({ bugs: 0, flags: 3, infos: 1 })).toBe(
      "3 Flags",
    );
    expect(problemSummaryTitle({ bugs: 0, flags: 0, infos: 1 })).toBe(
      "1 Informational",
    );
    expect(problemSummaryTitle({ bugs: 0, flags: 0, infos: 0 })).toBe(
      "No problems",
    );
  });
});

describe("countProblemKinds", () => {
  it("counts a bounded diagnostic list", () => {
    expect(countProblemKinds(["error", "warning", "warning", "info"])).toEqual({
      bugs: 1,
      flags: 2,
      infos: 1,
    });
  });
});
