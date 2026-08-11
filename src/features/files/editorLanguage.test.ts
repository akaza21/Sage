import { describe, expect, it } from "vitest";

import {
  editorLanguageForPath,
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
