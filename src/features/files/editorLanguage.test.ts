import { describe, expect, it } from "vitest";

import {
  editorLanguageForPath,
  editorLanguageStatusLabel,
  MAX_EDITOR_LANGUAGE_PATH_LENGTH,
} from "./editorLanguage";

describe("editorLanguageForPath", () => {
  it.each([
    ["src/app.ts", "typescript"],
    ["src/view.TSX", "typescript-jsx"],
    ["scripts/build.mjs", "javascript"],
    ["src/view.jsx", "javascript-jsx"],
    ["package.json", "json"],
    ["styles/app.css", "css"],
    ["public/index.html", "html"],
    ["docs/readme.md", "markdown"],
    ["tools/check.py", "python"],
    ["src/main.rs", "rust"],
  ] as const)("selects %s as %s", (path, expected) => {
    expect(editorLanguageForPath(path)).toBe(expected);
  });

  it("falls back to plain text for unknown, extensionless, dotfile, or oversized paths", () => {
    expect(editorLanguageForPath("archive.sage-unknown")).toBe("plain");
    expect(editorLanguageForPath("LICENSE")).toBe("plain");
    expect(editorLanguageForPath(".editorconfig")).toBe("plain");
    expect(
      editorLanguageForPath(
        `${"a".repeat(MAX_EDITOR_LANGUAGE_PATH_LENGTH)}.ts`,
      ),
    ).toBe("plain");
  });
});

describe("editorLanguageStatusLabel", () => {
  it("maps admitted File languages and hides plain or missing paths", () => {
    expect(editorLanguageStatusLabel("src/app.ts")).toBe("TypeScript");
    expect(editorLanguageStatusLabel("src/view.tsx")).toBe("TypeScript");
    expect(editorLanguageStatusLabel("scripts/build.mjs")).toBe("JavaScript");
    expect(editorLanguageStatusLabel("package.json")).toBe("JSON");
    expect(editorLanguageStatusLabel("docs/readme.md")).toBe("Markdown");
    expect(editorLanguageStatusLabel("LICENSE")).toBeNull();
    expect(editorLanguageStatusLabel("")).toBeNull();
    expect(editorLanguageStatusLabel(null)).toBeNull();
  });
});
