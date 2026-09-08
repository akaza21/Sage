import { describe, expect, it } from "vitest";

import { boundedError, errorMessage } from "./errors";

describe("shared domain errors", () => {
  it("extracts Error messages and stringifies non-Errors", () => {
    expect(errorMessage(new Error("expected"))).toBe("expected");
    expect(errorMessage({ code: 1 })).toBe("[object Object]");
  });

  it("keeps UTF-16 slicing deliberately distinct from UTF-8 truncation", () => {
    const surrogateBoundary = `${"x".repeat(2_047)}🚀`;
    expect(boundedError(surrogateBoundary, 2_048, "utf16")).toHaveLength(2_048);
    expect(
      boundedError(surrogateBoundary, 2_048, "utf16").endsWith("\ud83d"),
    ).toBe(true);
    expect(boundedError("🚀".repeat(1024), 2_048, "utf8")).toBe(
      "🚀".repeat(512),
    );
  });
});
