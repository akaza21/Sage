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
    expect(rootBlock).toMatch(/--color-canvas:\s*#0e1116;/);
    expect(rootBlock).toMatch(/--color-accent:\s*#4c8dff;/);
    expect(rootBlock).toMatch(/--color-accent-hover:\s*#7aa9ff;/);
    expect(rootBlock).toMatch(/--bg:\s*var\(--color-canvas\);/);
    expect(rootBlock).toMatch(/--accent:\s*var\(--color-accent\);/);
    expect(rootBlock).not.toMatch(/#9ad19b|#101210|#b5e4b4|#253c2a/);
    expect(themeSource).not.toMatch(/#9ad19b|#101210/);
  });

  it("defines a compact chrome ramp apart from Chat reading text", () => {
    expect(rootBlock).toMatch(/--font-size-xs:\s*11px;/);
    expect(rootBlock).toMatch(/--font-size-sm:\s*12px;/);
    expect(rootBlock).toMatch(/--font-size-md:\s*14px;/);
    expect(rootBlock).toMatch(/--font-size-xl:\s*20px;/);
    expect(rootBlock).toMatch(/--type-size-chrome:\s*var\(--font-size-sm\);/);
    expect(rootBlock).toMatch(/--type-size-chat:\s*var\(--font-size-md\);/);
  });
});
