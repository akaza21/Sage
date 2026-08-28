import { readFile } from "node:fs/promises";
import path from "node:path";

import { expect, it } from "vitest";

it("animates Thought disclosure only when motion is allowed", async () => {
  const css = await readFile(
    path.join(process.cwd(), "src/features/chat/acp/acp-chat.css"),
    "utf8",
  );

  expect(css).toMatch(
    /\.acp-thought\[open\] > :not\(summary\)[\s\S]*?animation: acp-thought-reveal/,
  );
  expect(css).toMatch(
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?animation-duration: 0s !important;[\s\S]*?animation-iteration-count: 1 !important;/,
  );
});
