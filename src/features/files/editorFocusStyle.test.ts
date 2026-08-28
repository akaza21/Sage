import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const editorStyles = readFileSync(
  resolve(process.cwd(), "src/styles/file-editor-surface.css"),
  "utf8",
);

describe("editor focus style policy", () => {
  it("keeps a token-based visible focus outline for editable and read-only editors", () => {
    const rule = editorStyles.match(
      /\.editor-input\s+\.cm-focused\s*\{(?<declarations>[^}]*)\}/,
    );

    expect(rule?.[0]).toBeDefined();
    expect(rule?.groups?.declarations).toMatch(
      /outline:\s*2px solid var\(--accent\);/,
    );
    expect(rule?.groups?.declarations).toMatch(/outline-offset:\s*-2px;/);
    expect(rule?.[0]).not.toContain("aria-readonly");
  });
});
