import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const editorStyles = readFileSync(
  resolve(process.cwd(), "src/styles/file-editor-surface.css"),
  "utf8",
);

describe("editor focus style policy", () => {
  it("keeps the shared focus frame for editable and read-only editors", () => {
    const rule = editorStyles.match(
      /\.editor-input\s+\.cm-focused\s*\{(?<declarations>[^}]*)\}/,
    );

    expect(rule?.[0]).toBeDefined();
    expect(rule?.groups?.declarations).toMatch(/outline:\s*0;/);
    expect(rule?.groups?.declarations).toMatch(
      /box-shadow:\s*var\(--highlight-focus-frame-shadow\);/,
    );
    expect(rule?.[0]).not.toContain("aria-readonly");
  });
});
