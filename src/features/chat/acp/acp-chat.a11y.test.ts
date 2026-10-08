import { readFile } from "node:fs/promises";
import path from "node:path";

import { expect, it } from "vitest";

it("keeps live thinking muted, clipped and nonanimated without disclosure styles", async () => {
  const css = await readFile(
    path.join(process.cwd(), "src/features/chat/acp/acp-activity-details.css"),
    "utf8",
  );

  const thoughtRule = css.match(/\.acp-thought \{([^}]+)\}/)?.[1];
  expect(thoughtRule).toBeDefined();
  expect(thoughtRule).toContain("color: var(--color-text-muted)");
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
