import { readFile } from "node:fs/promises";
import path from "node:path";

import { expect, it } from "vitest";

it("stops working-pulse animation when reduced motion is requested", async () => {
  const css = await readFile(
    path.join(process.cwd(), "src/styles/global.css"),
    "utf8",
  );

  expect(css).toMatch(
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?animation-duration: 0s !important;[\s\S]*?animation-iteration-count: 1 !important;/,
  );
});
