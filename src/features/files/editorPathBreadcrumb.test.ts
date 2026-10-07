import { describe, expect, it } from "vitest";

import {
  fileBreadcrumbSegments,
  MAX_BREADCRUMB_PATH_LENGTH,
} from "./FilePathChips";

describe("fileBreadcrumbSegments", () => {
  it("splits a short editor path into VS Code-like chips", () => {
    expect(
      fileBreadcrumbSegments("src/features/files/FileSurface.tsx"),
    ).toEqual(["src", "features", "files", "FileSurface.tsx"]);
  });

  it("collapses oversized paths instead of growing an unbounded chip list", () => {
    expect(fileBreadcrumbSegments("a/b/c/d/e/f/g/train.py")).toEqual([
      "a",
      "…",
      "e",
      "f",
      "g",
      "train.py",
    ]);
  });

  it("returns no chips for empty or oversized paths", () => {
    expect(fileBreadcrumbSegments("")).toEqual([]);
    expect(
      fileBreadcrumbSegments(`${"a/".repeat(MAX_BREADCRUMB_PATH_LENGTH)}x`),
    ).toEqual([]);
  });
});
