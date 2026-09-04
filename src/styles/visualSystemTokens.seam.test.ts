import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const visualSystemSource = readFileSync(
  resolve(process.cwd(), "src/styles/global.css"),
  "utf8",
);
const rootBlock = visualSystemSource.match(
  /:root\s*\{(?<declarations>[\s\S]*?)\n\}/,
)?.groups?.declarations;

describe("Wave 2 visual-system tokens", () => {
  it("freezes semantic color roles and maps the legacy surface names to them", () => {
    expect(rootBlock).toBeDefined();
    expect(rootBlock).toMatch(/--color-canvas:\s*#0e1116;/);
    expect(rootBlock).toMatch(/--color-panel:\s*#141820;/);
    expect(rootBlock).toMatch(/--color-surface:\s*#1a1f28;/);
    expect(rootBlock).toMatch(/--color-text-primary:\s*#e8edf5;/);
    expect(rootBlock).toMatch(/--color-status-foreground:\s*#fff;/);
    expect(rootBlock).toMatch(/--color-file-script:\s*#4c8dff;/);
    expect(rootBlock).toMatch(/--color-file-python:\s*#e2b66b;/);
    expect(rootBlock).toMatch(/--color-file-rust:\s*#ef9a9a;/);
    expect(rootBlock).toMatch(/--color-file-media:\s*#7aa9ff;/);
    expect(rootBlock).toMatch(/--color-accent:\s*#4c8dff;/);
    expect(rootBlock).toMatch(
      /--color-focus-ring:\s*var\(--color-accent-hover\);/,
    );
    expect(rootBlock).toMatch(/--color-success:\s*#68d983;/);
    expect(rootBlock).toMatch(/--color-diff-add:\s*var\(--color-success\);/);
    expect(rootBlock).toMatch(/--color-warning:\s*#e2b66b;/);
    expect(rootBlock).toMatch(/--color-danger:\s*#ef9a9a;/);
    expect(rootBlock).toMatch(/--color-overlay:\s*rgb\(0 0 0 \/ 0\.52\);/);
    expect(rootBlock).toMatch(/--color-diff-delete:\s*#ef6d67;/);
    expect(rootBlock).toMatch(/--color-file-data:\s*#d7c078;/);
    expect(rootBlock).toMatch(/--bg:\s*var\(--color-canvas\);/);
    expect(rootBlock).toMatch(/--surface:\s*var\(--color-panel\);/);
    expect(rootBlock).toMatch(/--text:\s*var\(--color-text-primary\);/);
    expect(rootBlock).toMatch(/--accent:\s*var\(--color-accent\);/);
  });

  it("uses one typography, four-pixel spacing, density, radius, and icon ramp", () => {
    expect(rootBlock).toMatch(/--font-size-xs:\s*11px;/);
    expect(rootBlock).toMatch(/--font-size-sm:\s*12px;/);
    expect(rootBlock).toMatch(/--font-size-md:\s*14px;/);
    expect(rootBlock).toMatch(/--font-size-lg:\s*16px;/);
    expect(rootBlock).toMatch(/--font-size-xl:\s*20px;/);
    expect(rootBlock).toMatch(/--space-1:\s*4px;/);
    expect(rootBlock).toMatch(/--space-2:\s*8px;/);
    expect(rootBlock).toMatch(/--space-3:\s*12px;/);
    expect(rootBlock).toMatch(/--space-4:\s*16px;/);
    expect(rootBlock).toMatch(/--space-6:\s*24px;/);
    expect(rootBlock).toMatch(/--space-8:\s*32px;/);
    expect(rootBlock).toMatch(/--control-height-compact:\s*28px;/);
    expect(rootBlock).toMatch(/--control-height-default:\s*32px;/);
    expect(rootBlock).toMatch(/--control-height-emphasis:\s*36px;/);
    expect(rootBlock).toMatch(/--radius-sm:\s*6px;/);
    expect(rootBlock).toMatch(/--radius-md:\s*8px;/);
    expect(rootBlock).toMatch(/--radius-lg:\s*12px;/);
    expect(rootBlock).toMatch(/--radius-pill:\s*999px;/);
    expect(rootBlock).toMatch(/--radius-none:\s*0;/);
    expect(rootBlock).toMatch(/--radius-circle:\s*50%;/);
    expect(rootBlock).toMatch(/--icon-size-sm:\s*14px;/);
    expect(rootBlock).toMatch(/--icon-size-md:\s*16px;/);
    expect(rootBlock).toMatch(/--icon-size-lg:\s*20px;/);
  });

  it("defines one focus, elevation, disabled, and motion contract", () => {
    expect(rootBlock).toMatch(/--highlight-focus-edge:\s*color-mix\(/);
    expect(rootBlock).toMatch(/--highlight-focus-halo:\s*color-mix\(/);
    expect(rootBlock).toMatch(/--highlight-selection-fill:\s*color-mix\(/);
    expect(rootBlock).toMatch(/--highlight-selection-edge:\s*color-mix\(/);
    expect(rootBlock).toMatch(/--highlight-active-fill:\s*color-mix\(/);
    expect(rootBlock).toMatch(/--highlight-active-edge:\s*color-mix\(/);
    expect(rootBlock).toMatch(
      /--highlight-focus-halo-shadow:\s*0 0 6px 1px var\(--highlight-focus-halo\);/,
    );
    expect(rootBlock).toMatch(
      /--highlight-focus-frame-shadow:\s*inset 0 0 0 1px var\(--highlight-focus-edge\),\s*inset 0 0 6px var\(--highlight-focus-halo\);/,
    );
    expect(rootBlock).toMatch(/--focus-ring-width:\s*1px;/);
    expect(rootBlock).toMatch(/--focus-ring-offset:\s*1px;/);
    expect(rootBlock).toMatch(
      /--focus-ring:\s*var\(--focus-ring-width\) solid var\(--highlight-focus-edge\);/,
    );
    expect(rootBlock).toMatch(
      /--focus-ring-inset:\s*inset 0 0 0 var\(--focus-ring-width\)\s*var\(--highlight-focus-edge\);/,
    );
    expect(rootBlock).toMatch(/--opacity-disabled:\s*0\.45;/);
    expect(rootBlock).toMatch(/--motion-duration-fast:\s*100ms;/);
    expect(rootBlock).toMatch(/--motion-duration-normal:\s*160ms;/);
    expect(rootBlock).toMatch(/--elevation-popover:/);
    expect(rootBlock).toMatch(/--elevation-floating:/);
    expect(visualSystemSource).toMatch(
      /:where\([\s\S]*\[tabindex\]:not\(\[tabindex="-1"\]\)[\s\S]*\):focus-visible[\s\S]*outline:\s*var\(--focus-ring\);[\s\S]*outline-offset:\s*var\(--focus-ring-offset\);/,
    );
    expect(visualSystemSource).toMatch(
      /\[aria-disabled="true"\][\s\S]*opacity:\s*var\(--opacity-disabled\);/,
    );
  });

  it("publishes effective-viewport, reduced-motion, and forced-color behavior", () => {
    expect(visualSystemSource).toMatch(
      /@media \(max-width: 960px\)[\s\S]*--reading-measure:\s*var\(--reading-measure-compact\);/,
    );
    expect(visualSystemSource).toMatch(
      /@media \(prefers-reduced-motion: reduce\)[\s\S]*--motion-duration-fast:\s*0ms;[\s\S]*--motion-duration-normal:\s*0ms;/,
    );
    expect(visualSystemSource).toMatch(
      /@media \(forced-colors: active\)[\s\S]*--color-canvas:\s*Canvas;[\s\S]*--color-focus-ring:\s*Highlight;[\s\S]*--highlight-focus-edge:\s*Highlight;[\s\S]*--highlight-focus-halo-shadow:\s*none;[\s\S]*--focus-ring-width:\s*2px;[\s\S]*--elevation-popover:\s*none;/,
    );
  });
});
