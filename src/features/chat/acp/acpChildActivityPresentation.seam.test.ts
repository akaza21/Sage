import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

async function readStylesheet(relativePath: string) {
  return readFile(resolve(relativePath), "utf8");
}

describe("ACP child activity presentation stylesheet seam", () => {
  it("owns a compact four-level hierarchy and preserves full labels", async () => {
    const [projection, css] = await Promise.all([
      readStylesheet("src/features/chat/acp/AcpChatProjection.tsx"),
      readStylesheet("src/features/chat/acp/acp-child-activity.css"),
    ]);

    expect(projection).toContain('import "./acp-child-activity.css";');
    expect(css).toMatch(
      /\.acp-child-activity__summary\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\) auto;/s,
    );
    expect(css).toMatch(
      /\.acp-child-activity__label,[\s\S]*?\.acp-collaboration__label\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/,
    );
    for (const level of [2, 3, 4]) {
      expect(css).toContain(`.acp-child-activity__item[aria-level="${level}"]`);
    }
    expect(css).toMatch(
      /\.acp-child-activity__item::before\s*\{[^}]*border-block-start:\s*1px solid var\(--color-border-strong\);/s,
    );
    expect(css).toContain('[data-lifecycle="active"]');
    expect(css).toContain('[data-lifecycle="streaming"]');
    expect(css).toContain('[data-lifecycle="failed"]');
    expect(css).not.toContain('[aria-label="Active"]');
    expect(css).not.toContain('[aria-label="Failed"]');
    expect(css).not.toMatch(/animation|transition|scale/);
  });

  it("keeps the hierarchy visible in narrow and forced-color modes", async () => {
    const css = await readStylesheet(
      "src/features/chat/acp/acp-child-activity.css",
    );

    expect(css).toMatch(
      /@media \(max-width: 640px\)[\s\S]*?\.acp-child-activity__tree\s*\{[^}]*--acp-child-indent:\s*var\(--space-3\);/,
    );
    expect(css).toMatch(
      /@media \(forced-colors: active\)[\s\S]*?\.acp-child-activity\s*\{[^}]*border-inline-start-color:\s*CanvasText;/,
    );
    expect(css).toMatch(
      /@media \(forced-colors: active\)[\s\S]*?\.acp-child-activity__item::before\s*\{[^}]*border-block-start-color:\s*CanvasText;/,
    );
  });
});
