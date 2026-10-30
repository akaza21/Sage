import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const read = (path: string) => readFile(resolve(path), "utf8");

function ruleBody(css: string, selector: string) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`(?:^|\\n)${escaped}\\s*\\{([^}]*)\\}`))?.[1];
}

describe("Editor chrome alignment seam", () => {
  it("keeps tab icons at full size when the title shrinks (ED-8)", async () => {
    const chrome = await read("src/styles/workbench-chrome.css");
    expect(ruleBody(chrome, ".tab__activate > svg")).toMatch(/flex:\s*none;/);
  });

  it("runs the active-line band from the gutter to the right edge (ED-10)", async () => {
    const surface = await read("src/styles/file-editor-surface.css");
    expect(ruleBody(surface, ".editor-input .cm-content")).toMatch(
      /padding:\s*16px 0;/,
    );
    expect(ruleBody(surface, ".editor-input .cm-line")).toMatch(
      /padding:\s*0 20px 0 8px;/,
    );
  });

  it("keeps the header metadata on the right whether or not actions follow (ED-13)", async () => {
    const surface = await read("src/styles/file-editor-surface.css");
    expect(ruleBody(surface, ".file-header__metadata")).toMatch(
      /margin-left:\s*auto;/,
    );
  });

  it("lines the repository summary up with the group labels (ED-11)", async () => {
    const reference = await read("src/styles/editor-visual-reference.css");
    expect(
      ruleBody(reference, ".workbench--editor .source-root__summary"),
    ).toMatch(/padding:\s*4px 4px 4px 8px;/);
  });

  it("draws a disabled Commit without the accent (ED-12)", async () => {
    const reference = await read("src/styles/editor-visual-reference.css");
    const disabled = ruleBody(
      reference,
      ".workbench--editor .repository-commit__actions button:disabled",
    );
    expect(disabled).toBeDefined();
    expect(disabled).not.toMatch(/accent/);
    expect(disabled).toMatch(/opacity:\s*1;/);
    expect(disabled).toMatch(/color:\s*var\(--color-text-muted\);/);
  });
});
