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

  // ED-9: the frame used to be an inset shadow on the editor, which the
  // sticky gutter (z-index 200) painted over along the left edge.
  it("draws the focus frame above the gutter without taking clicks", () => {
    const frame = editorStyles.match(
      /\.editor-input\s+\.cm-editor\.cm-focused::after\s*\{(?<declarations>[^}]*)\}/,
    )?.groups?.declarations;

    expect(frame).toMatch(/position:\s*absolute;/);
    expect(frame).toMatch(/inset:\s*0;/);
    expect(Number(frame?.match(/z-index:\s*(\d+);/)?.[1])).toBeGreaterThan(200);
    expect(frame).toMatch(/pointer-events:\s*none;/);
    expect(frame).toMatch(/border-radius:/);
  });
});
