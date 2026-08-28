import { describe, expect, it } from "vitest";

import { fileKindForPath } from "./fileKind";

describe("fileKindForPath", () => {
  it.each([
    ["src/app.ts", "file", "typescript"],
    ["src/view.tsx", "file", "typescript"],
    ["scripts/build.mjs", "file", "javascript"],
    ["tools/check.py", "file", "python"],
    ["src/main.rs", "file", "rust"],
    ["docs/readme.md", "file", "markdown"],
    ["package.json", "file", "json"],
    ["uv.lock", "file", "lock"],
    ["progress.png", "file", "image"],
    ["analysis.ipynb", "file", "notebook"],
    ["results.tsv", "file", "table"],
    ["pyproject.toml", "file", "toml"],
    ["src", "directory", "folder"],
    ["vendor/lib", "symlink", "symlink"],
  ] as const)("maps %s (%s) to %s", (path, entryKind, expected) => {
    expect(fileKindForPath(path, entryKind)).toBe(expected);
  });

  it("falls back to plain for unknown or oversized paths", () => {
    expect(fileKindForPath("LICENSE")).toBe("plain");
    expect(fileKindForPath("archive.sage-unknown")).toBe("plain");
  });
});
