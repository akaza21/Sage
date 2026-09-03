import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

describe("Problems surface stylesheet seam", () => {
  it("loads Problems rules from a focused stylesheet, not the shell stylesheet", async () => {
    const [entry, shellCss, surfaceCss] = await Promise.all([
      readFile(resolve("src/main.tsx"), "utf8"),
      readFile(resolve("src/styles/global.css"), "utf8"),
      readFile(resolve("src/styles/problems-outline-surfaces.css"), "utf8"),
    ]);

    expect(entry).toMatch(
      /import ["']\.\/styles\/problems-outline-surfaces\.css["'];/,
    );
    expect(shellCss).not.toMatch(/^\.problems-surface\b/m);
    expect(surfaceCss).toMatch(/^\.problems-surface\b/m);
    expect(surfaceCss).toMatch(/^\.problem-card\b/m);
  });

  it("keeps Problems and Outline rows dense, quiet, and truncation-safe", async () => {
    const surfaceCss = await readFile(
      resolve("src/styles/problems-outline-surfaces.css"),
      "utf8",
    );

    expect(surfaceCss).toMatch(
      /\.problem-card,[\s\S]*\.outline-card\s*\{[^}]*display:\s*grid;[^}]*min-height:\s*28px;[^}]*grid-template-columns:\s*16px minmax\(0, 1fr\);[^}]*padding:\s*3px 7px;/s,
    );
    expect(surfaceCss).toMatch(
      /\.problem-card__kind,[\s\S]*\.outline-card__kind\s*\{[^}]*width:\s*16px;[^}]*height:\s*16px;[^}]*background:\s*transparent;/s,
    );
    expect(surfaceCss).toMatch(
      /\.problem-card__message,[\s\S]*\.outline-card__name\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/s,
    );
    expect(surfaceCss).toMatch(
      /\.problem-card__location-file,[\s\S]*\.outline-card__location-file\s*\{[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;/s,
    );
    expect(surfaceCss).toMatch(
      /\.problem-card__location-position,[\s\S]*\.outline-card__location-position\s*\{[^}]*flex:\s*0 0 auto;/s,
    );
    expect(surfaceCss).toContain("@media (forced-colors: active)");
  });
});
