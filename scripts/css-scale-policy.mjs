import { readFile } from "node:fs/promises";
import path from "node:path";

const TYPE_SCALE_AUTHORITY = "src/styles/global.css";
// Sizes off the type scale, each drawn once for one glyph.
const FONT_SIZE_EXCEPTIONS = new Map([
  ["src/features/browser/BrowserNewTab.css:10px", "Shortcut tile badge."],
  ["src/features/browser/BrowserNewTab.css:25px", "New-tab hero mark."],
  ["src/styles/shared-dialogs.css:18px", "Dialog title."],
]);
/** Window widths the app adapts at; a pane uses a container query. */
export const WINDOW_BREAKPOINTS = new Set([640, 760, 960]);

function stripComments(source) {
  return source.replace(/\/\*[\s\S]*?(?:\*\/|$)/gu, (match) =>
    match.replace(/[^\n]/gu, " "),
  );
}

const lineAt = (source, offset) => source.slice(0, offset).split("\n").length;

/**
 * Font sizes come from the `--font-size-*` scale, and `@media` widths from
 * the shared window breakpoints, so a size or breakpoint changes in one place.
 */
export function offScaleDeclarations(files) {
  const findings = [];
  for (const { relativePath, source } of files) {
    const css = stripComments(source);
    if (relativePath !== TYPE_SCALE_AUTHORITY) {
      for (const match of css.matchAll(
        /(?<![\w-])font-size\s*:\s*(-?[\d.]+px)\b/giu,
      )) {
        if (FONT_SIZE_EXCEPTIONS.has(`${relativePath}:${match[1]}`)) continue;
        findings.push({
          line: lineAt(css, match.index),
          message: `font-size ${match[1]}; use a var(--font-size-*) token`,
          relativePath,
        });
      }
    }
    for (const media of css.matchAll(/@media\b[^{]*/giu)) {
      for (const width of media[0].matchAll(
        /(?:min|max)-width\s*:\s*([\d.]+)px/giu,
      )) {
        if (WINDOW_BREAKPOINTS.has(Number(width[1]))) continue;
        findings.push({
          line: lineAt(css, media.index),
          message: `@media width ${width[1]}px; use ${[...WINDOW_BREAKPOINTS].join(", ")}px or a container query`,
          relativePath,
        });
      }
    }
  }
  return findings;
}

export async function cssScalePolicyErrors(root, cssFiles) {
  const files = await Promise.all(
    cssFiles.map(async (relativePath) => ({
      relativePath: relativePath.split(path.sep).join("/"),
      source: await readFile(path.join(root, relativePath), "utf8"),
    })),
  );
  return offScaleDeclarations(files).map(
    ({ line, message, relativePath }) => `${relativePath}:${line} ${message}.`,
  );
}
