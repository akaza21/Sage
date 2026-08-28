import { describe, expect, it } from "vitest";

import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";

describe("outlineKindLabel", () => {
  it("maps outline kinds onto Devin card labels", () => {
    expect(outlineKindLabel("function")).toBe("Function");
    expect(outlineKindLabel("type")).toBe("Type");
    expect(outlineKindLabel("file")).toBe("File");
    expect(outlineKindLabel("unknown")).toBe("Symbol");
  });
});

describe("outlineSummaryTitle", () => {
  it("uses a compact symbol count heading", () => {
    expect(outlineSummaryTitle(0)).toBe("No symbols");
    expect(outlineSummaryTitle(1)).toBe("1 Symbol");
    expect(outlineSummaryTitle(3)).toBe("3 Symbols");
  });
});
