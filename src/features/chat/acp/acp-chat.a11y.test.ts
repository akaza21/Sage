import { readFile } from "node:fs/promises";
import path from "node:path";

import { expect, it } from "vitest";

it("keeps live thinking clipped without masking its shimmer, with accessible motion fallbacks", async () => {
  const css = await readFile(
    path.join(process.cwd(), "src/features/chat/acp/acp-activity-details.css"),
    "utf8",
  );

  const thoughtRule = css.match(/\.acp-thought \{([^}]+)\}/)?.[1];
  expect(thoughtRule).toBeDefined();
  expect(thoughtRule).not.toMatch(/(?:^|[;\s])color:/);
  const shimmerRule = css.match(/\.acp-live-shimmer \{([^}]+)\}/)?.[1];
  expect(shimmerRule).toContain("color: transparent");
  expect(shimmerRule).toContain("background-clip: text");
  expect(shimmerRule).toContain("animation: acp-live-shimmer");
  expect(css).toMatch(
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.acp-live-shimmer \{[^}]*color: var\(--color-text-muted\)/,
  );
  expect(css).toMatch(
    /@media \(forced-colors: active\)[\s\S]*?\.acp-live-shimmer \{[^}]*color: CanvasText/,
  );
  expect(thoughtRule).toContain("overflow: hidden");
  expect(thoughtRule).toContain("text-overflow: ellipsis");
  expect(thoughtRule).toContain("white-space: nowrap");
  expect(thoughtRule).not.toMatch(/animation|transition/);
  expect(css).not.toContain(".acp-thought[open]");
  expect(css).not.toContain(".acp-thought__");
  expect(css).not.toContain("acp-thought-reveal");
  expect(css).toMatch(
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.acp-tool-rollup__chevron \{\s*transition: none;/,
  );
});
