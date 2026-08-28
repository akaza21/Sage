import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const themeSource = readFileSync(
  resolve(process.cwd(), "src/styles/global.css"),
  "utf8",
);
const rootBlock = themeSource.match(/:root\s*\{(?<declarations>[^}]*)\}/)
  ?.groups?.declarations;

describe("Devin-dark theme tokens", () => {
  it("uses a dark cool-neutral canvas and a blue accent, not sage-green", () => {
    expect(rootBlock).toBeDefined();
    expect(rootBlock).toMatch(/--bg:\s*#0e1116;/);
    expect(rootBlock).toMatch(/--accent:\s*#4c8dff;/);
    expect(rootBlock).toMatch(/--accent-strong:\s*#7aa9ff;/);
    expect(rootBlock).not.toMatch(/#9ad19b|#101210|#b5e4b4|#253c2a/);
    expect(themeSource).not.toMatch(/#9ad19b|#101210/);
  });

  it("defines a compact chrome ramp apart from Chat reading text", () => {
    expect(rootBlock).toMatch(/--type-size-chrome:\s*12px;/);
    expect(rootBlock).toMatch(/--type-size-caption:\s*11px;/);
    expect(rootBlock).toMatch(/--type-size-chat:\s*14px;/);
    expect(rootBlock).toMatch(/--type-size-chat-title:\s*20px;/);
    expect(rootBlock).toMatch(/--type-size-editor-ui:\s*12px;/);
  });
});
