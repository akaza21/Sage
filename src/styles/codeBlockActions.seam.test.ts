import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const source = readFileSync(
  resolve("src/features/chat/acp/acp-chat.css"),
  "utf8",
);
const ordinarySource = source.slice(
  0,
  source.indexOf("@media (forced-colors: active)"),
);
const forcedColorsSource = source.slice(
  source.indexOf("@media (forced-colors: active)"),
);

describe("Code block action stylesheet seam", () => {
  it("keeps hover and keyboard focus distinct from the panel without claiming active state", () => {
    expect(ordinarySource).toMatch(
      /\.acp-code-block\s*\{[^}]*background:\s*var\(--color-surface-hover\);/s,
    );
    expect(ordinarySource).toMatch(
      /\.acp-code-block__header\s+button:is\(:hover, :focus-visible\)\s*\{[^}]*background:\s*var\(--color-surface\);/s,
    );
    expect(ordinarySource).toMatch(
      /\.acp-code-block__header\s+button:is\(\[aria-pressed="true"\], \[data-copy-status\]\)\s*\{[^}]*background:\s*var\(--color-surface-active\);/s,
    );
    expect(
      ordinarySource.indexOf(
        'button:is([aria-pressed="true"], [data-copy-status])',
      ),
    ).toBeGreaterThan(
      ordinarySource.indexOf("button:is(:hover, :focus-visible)"),
    );
    const theme = readFileSync(resolve("src/styles/global.css"), "utf8");
    const actionFill = theme.match(/--color-surface:\s*([^;]+);/)?.[1];
    const panelFill = theme.match(/--color-surface-hover:\s*([^;]+);/)?.[1];
    expect(actionFill).toBeDefined();
    expect(panelFill).toBeDefined();
    expect(actionFill).not.toBe(panelFill);
  });

  it("gives the masked wrap icon system foregrounds at rest and in highlighted action states", () => {
    expect(ordinarySource).toMatch(
      /\.acp-code-block__wrap-icon\s*\{[^}]*background:\s*currentColor;/s,
    );
    expect(ordinarySource).toMatch(
      /\.acp-code-block__wrap-icon\s*\{[^}]*mask:\s*url\("\.\/code-wrap\.svg"\) center \/ contain no-repeat;/s,
    );
    expect(forcedColorsSource).toMatch(
      /\.acp-code-block__wrap-icon\s*\{[^}]*forced-color-adjust:\s*none;[^}]*color:\s*ButtonText;/s,
    );
    expect(forcedColorsSource).toMatch(
      /\.acp-code-block__header\s+button:is\(\s*:hover,\s*:focus-visible,\s*\[aria-pressed="true"\],\s*\[data-copy-status\]\s*\)\s+\.acp-code-block__wrap-icon\s*\{[^}]*color:\s*HighlightText;/s,
    );
  });
});
