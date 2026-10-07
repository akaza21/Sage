import { readFile, readdir } from "node:fs/promises";
import { join, resolve } from "node:path";

import { describe, expect, it } from "vitest";

const sizingProperties = new Set([
  "gap",
  "row-gap",
  "column-gap",
  "height",
  "min-height",
  "max-height",
  "padding",
  "padding-top",
  "padding-right",
  "padding-bottom",
  "padding-left",
]);

const flowProperties = new Set([
  "margin",
  "margin-top",
  "margin-right",
  "margin-bottom",
  "margin-left",
]);

const paddingProperties = new Set([
  "padding",
  "padding-top",
  "padding-right",
  "padding-bottom",
  "padding-left",
]);

const declaration = /^\s*([a-z-]+)\s*:\s*([^;]+);/;
const pixelLength = /(\d*\.?\d+)px/g;

async function uiStylesheets(root: string): Promise<string[]> {
  const entries = await readdir(root, { withFileTypes: true });
  const found: string[] = [];
  for (const entry of entries) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await uiStylesheets(path)));
      continue;
    }
    if (entry.name.endsWith(".css")) {
      found.push(path);
    }
  }
  return found.sort();
}

function offScaleValue(property: string, pixels: number): boolean {
  if (!Number.isInteger(pixels)) {
    return true;
  }
  if (pixels === 0 || pixels === 2) {
    return false;
  }
  if (pixels === 1) {
    return paddingProperties.has(property);
  }
  return pixels % 4 !== 0;
}

describe("spacing scale seam", () => {
  it("keeps every sizing literal on the four pixel scale with hairlines as the only odd value", async () => {
    const files = await uiStylesheets(resolve("src"));
    expect(files.length).toBeGreaterThan(30);

    const offenders: string[] = [];
    for (const file of files) {
      const source = await readFile(file, "utf8");
      const lines = source.split(/\r?\n/);
      lines.forEach((line, index) => {
        const parsed = declaration.exec(line);
        if (!parsed) {
          return;
        }
        const [, property, value] = parsed;
        if (!sizingProperties.has(property) && !flowProperties.has(property)) {
          return;
        }
        for (const match of value.matchAll(pixelLength)) {
          const pixels = Number(match[1]);
          if (offScaleValue(property, pixels)) {
            offenders.push(
              `${file.replace(resolve("src"), "src")}:${index + 1} ${property}: ${value.trim()}`,
            );
          }
        }
      });
    }

    expect(offenders).toEqual([]);
  });
});
