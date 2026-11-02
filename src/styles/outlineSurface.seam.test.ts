import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Outline surface stylesheet seam", () => {
  it("keeps Outline rows dense on the four pixel scale, quiet, and truncation-safe", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/outline-surface.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.outline-card\s*\{[^}]*display:\s*grid;[^}]*min-height:\s*28px;[^}]*grid-template-columns:\s*16px minmax\(0, 1fr\);[^}]*padding:\s*4px 8px;/s,
    );
    expect(surfaceCss).toMatch(
      /\.outline-card__kind\s*\{[^}]*width:\s*16px;[^}]*height:\s*16px;[^}]*background:\s*transparent;/s,
    );
    expect(surfaceCss).toMatch(
      /\.outline-card__name\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
    );
    expect(surfaceCss).toMatch(
      /\.outline-card__location-file\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;/s,
    );
    expect(surfaceCss).toMatch(
      /\.outline-card__location-position\s*\{[^}]*flex:\s*0 0 auto;/s,
    );
    expect(surfaceCss).toContain("@media (forced-colors: active)");
  });
});
