import { describe, expect, it } from "vitest";

import type { LanguageOutlineReport } from "../../domain/languageService";
import { localSymbolNavigation } from "./outlineNavigation";

const resource = { rootId: "root-a", relativePath: "src/example.ts" };
const content = "const value = example();\n\nexport function example() {}\n";

function report(
  symbols: LanguageOutlineReport["symbols"],
): LanguageOutlineReport {
  return { resource, generation: 1, symbols };
}

describe("localSymbolNavigation", () => {
  it("admits only a same-resource outline declaration and navigates the File caret", () => {
    expect(
      localSymbolNavigation(
        resource,
        content,
        { start: 14, end: 14 },
        report([{ resource, name: "example", kind: "function", startLine: 3 }]),
      ),
    ).toEqual({ start: 25, end: 25 });
  });

  it("fails closed when the language process has not delivered an outline report", () => {
    expect(
      localSymbolNavigation(resource, content, { start: 14, end: 14 }, null),
    ).toBeNull();
  });

  it("rejects an over-cap report instead of searching unbounded symbols", () => {
    const symbols = Array.from({ length: 65 }, (_, index) => ({
      resource,
      name: index === 64 ? "example" : `other${index}`,
      kind: "function" as const,
      startLine: 3,
    }));
    expect(
      localSymbolNavigation(
        resource,
        content,
        { start: 14, end: 14 },
        report(symbols),
      ),
    ).toBeNull();
  });
});
