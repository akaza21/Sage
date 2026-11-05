import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Editor Activity Bar stylesheet seam", () => {
  it("sizes the rail target from the emphasis control token rather than a loose literal", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/editor-activity-bar.css"),
      "utf8",
    );
    expect(surfaceCss).toMatch(
      /\.activity-rail__button\s*\{[^}]*height:\s*var\(--control-height-emphasis\);/s,
    );
  });
});
