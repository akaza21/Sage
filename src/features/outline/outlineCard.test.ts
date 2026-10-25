import { describe, expect, it } from "vitest";

import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";

describe("outlineKindLabel", () => {
  it("maps every syntax Outline kind onto a card label", () => {
    expect(outlineKindLabel("function")).toBe("Function");
    expect(outlineKindLabel("class")).toBe("Class");
    expect(outlineKindLabel("type")).toBe("Type");
    expect(outlineKindLabel("variable")).toBe("Variable");
    expect(outlineKindLabel("module")).toBe("Module");
    expect(outlineKindLabel("heading")).toBe("Heading");
    expect(outlineKindLabel("property")).toBe("Property");
    expect(outlineKindLabel("rule")).toBe("Rule");
  });
});

describe("outlineSummaryTitle", () => {
  it("uses a compact symbol count heading", () => {
    expect(outlineSummaryTitle(0)).toBe("No symbols");
    expect(outlineSummaryTitle(1)).toBe("1 Symbol");
    expect(outlineSummaryTitle(3)).toBe("3 Symbols");
  });

  it("says when the list stops at its cap", () => {
    expect(outlineSummaryTitle(500, true)).toBe("First 500 Symbols");
  });
});
