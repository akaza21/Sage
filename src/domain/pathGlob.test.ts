import { readFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { MAX_PATH_GLOB_BYTES, validPathGlob } from "./pathGlob";
import { pathGlobMatcher, pathGlobMatches } from "./pathGlobMatcher";

interface Cases {
  valid: string[];
  invalid: string[];
  matches: { pattern: string; path: string; expected: boolean }[];
}

// The same table drives crates/sage-workspace/src/path_glob/tests.rs.
const cases = JSON.parse(
  readFileSync(
    path.resolve(
      __dirname,
      "../../crates/sage-workspace/src/path_glob/cases.json",
    ),
    "utf8",
  ),
) as Cases;

describe("path globs", () => {
  it("agrees with the Rust dialect on which patterns are valid", () => {
    for (const pattern of cases.valid)
      expect(validPathGlob(pattern)).toBe(true);
    for (const pattern of cases.invalid) {
      expect(validPathGlob(pattern)).toBe(false);
    }
    expect(validPathGlob("a".repeat(MAX_PATH_GLOB_BYTES))).toBe(true);
    expect(validPathGlob("a".repeat(MAX_PATH_GLOB_BYTES + 1))).toBe(false);
    expect(validPathGlob(7)).toBe(false);
  });

  it("agrees with the Rust dialect on every shared match case", () => {
    for (const { pattern, path: target, expected } of cases.matches) {
      expect(pathGlobMatches(pattern, target), `${pattern} ${target}`).toBe(
        expected,
      );
      expect(
        pathGlobMatcher([pattern])(target),
        `compiled ${pattern} ${target}`,
      ).toBe(expected);
    }
  });

  it("never backtracks exponentially on many stars or globstars", () => {
    const started = performance.now();
    expect(pathGlobMatches("**/*a*a*a*a*a*a*a*a*b", "a".repeat(200))).toBe(
      false,
    );
    expect(
      pathGlobMatches(`${"**/".repeat(12)}zz`, Array(40).fill("d").join("/")),
    ).toBe(false);
    expect(performance.now() - started).toBeLessThan(250);
  });

  it("builds one matcher per list and skips invalid patterns", () => {
    const patterns = Object.freeze([
      "**/.git",
      "**/*.tmp",
      "build/**",
      "/absolute",
    ]);
    const matcher = pathGlobMatcher(patterns);
    expect(pathGlobMatcher(patterns)).toBe(matcher);
    expect(matcher("x/.git")).toBe(true);
    expect(matcher("a.tmp")).toBe(true);
    expect(matcher("build/out/a.js")).toBe(true);
    expect(matcher("absolute")).toBe(false);
    expect(matcher("src/main.rs")).toBe(false);
    expect(pathGlobMatcher([])("anything")).toBe(false);
  });
});
