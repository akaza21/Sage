import { readFile } from "node:fs/promises";
import path from "node:path";

/** Blanks comments and quoted strings so neither can define or read a
 * custom property, keeping offsets and newlines for line numbers. */
function maskCss(source) {
  return source
    .replace(/\/\*[\s\S]*?(?:\*\/|$)/gu, (match) =>
      match.replace(/[^\n]/gu, " "),
    )
    .replace(/(["'])(?:\\[\s\S]|(?!\1)[^\\\n])*\1/gu, (match) =>
      match.replace(/[^\n]/gu, " "),
    );
}

function lineAt(source, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) {
    if (source[index] === "\n") line += 1;
  }
  return line;
}

const DEFINITION = /(?:^|[{;\s])(--[a-z0-9_-]+)\s*:/giu;
// A read with a fallback (`var(--x, …)`) is allowed to stay undefined.
const READ_WITHOUT_FALLBACK = /var\(\s*(--[a-z0-9_-]+)\s*\)/giu;
// Script sets a property by its literal name: `style={{ "--w": … }}` or
// `style.setProperty("--w", …)`.
const SCRIPT_NAME = /["'`](--[a-z0-9_-]+)["'`]/giu;

/**
 * Every `var(--x)` with no fallback must name a property some stylesheet
 * defines, or one the app sets from script by its literal name. A read of an
 * undefined property computes to the initial value, so the declaration
 * silently does nothing (a border that never draws).
 */
export function undefinedCustomProperties(cssFiles, scriptFiles) {
  const defined = new Set();
  const reads = [];
  for (const { relativePath, source } of cssFiles) {
    const masked = maskCss(source);
    for (const match of masked.matchAll(DEFINITION)) defined.add(match[1]);
    for (const match of masked.matchAll(READ_WITHOUT_FALLBACK)) {
      reads.push({
        line: lineAt(masked, match.index),
        name: match[1],
        relativePath,
      });
    }
  }
  for (const { source } of scriptFiles) {
    for (const match of source.matchAll(SCRIPT_NAME)) defined.add(match[1]);
  }
  return reads
    .filter(({ name }) => !defined.has(name))
    .sort(
      (left, right) =>
        left.relativePath.localeCompare(right.relativePath) ||
        left.line - right.line,
    );
}

async function readAll(root, relativeFiles) {
  return Promise.all(
    relativeFiles.map(async (relativePath) => ({
      relativePath: relativePath.split(path.sep).join("/"),
      source: await readFile(path.join(root, relativePath), "utf8"),
    })),
  );
}

export async function cssTokenPolicyErrors(root, cssFiles, scriptFiles) {
  const undefinedReads = undefinedCustomProperties(
    await readAll(root, cssFiles),
    await readAll(root, scriptFiles),
  );
  return undefinedReads.map(
    ({ line, name, relativePath }) =>
      `${relativePath}:${line} reads var(${name}), which no stylesheet defines and no script sets; define it, use an existing token, or give var() a fallback.`,
  );
}
