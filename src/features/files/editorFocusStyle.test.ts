import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const editorStyles = readFileSync(
  resolve(process.cwd(), "src/styles/file-editor-surface.css"),
  "utf8",
);

// Whether the frame is drawn above the gutter without taking clicks is
// measured in Chromium by tests/ui/editor-focus-frame.visual.ts.
describe("editor focus style policy", () => {
  it("keeps the shared focus frame for editable and read-only editors", () => {
    const rule = editorStyles.match(
      /\.editor-input\s+\.cm-focused\s*\{(?<declarations>[^}]*)\}/,
    );
    const frame = editorStyles.match(
      /\.editor-input\s+\.cm-editor\.cm-focused::after\s*\{(?<declarations>[^}]*)\}/,
    );

    expect(rule?.[0]).toBeDefined();
    expect(rule?.groups?.declarations).toMatch(/outline:\s*0;/);
    expect(frame?.groups?.declarations).toMatch(
      /box-shadow:\s*var\(\s*--editor-input-focus-frame,\s*var\(--highlight-focus-frame-shadow\)\s*\);/,
    );
    expect(rule?.[0]).not.toContain("aria-readonly");
    expect(frame?.[0]).not.toContain("aria-readonly");
  });
});
