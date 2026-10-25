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

// Measured Codex values that the 4 px scale cannot hold, each with its reason.
const exceptions = new Set([
  // Codex s2/f0093 selection pill (W9 B5): 8 px overflows the 480 px column.
  "src/features/chat/transcript-interactions.css .chat-selection-popover button padding: 0 7px",
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
      let selector = "";
      lines.forEach((line, index) => {
        if (line.trim().endsWith("{"))
          selector = line.trim().slice(0, -1).trim();
        const parsed = declaration.exec(line);
        if (!parsed) {
          return;
        }
        const [, property, value] = parsed;
        if (!sizingProperties.has(property) && !flowProperties.has(property)) {
          return;
        }
        const relative = file
          .replace(resolve("src"), "src")
          .replaceAll("\\", "/");
        if (
          exceptions.has(`${relative} ${selector} ${property}: ${value.trim()}`)
        )
          return;
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
