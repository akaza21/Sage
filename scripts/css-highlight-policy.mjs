import { lstat, readFile, readdir } from "node:fs/promises";
import path from "node:path";

export const CSS_HIGHLIGHT_MAX_FILES = 128;
export const CSS_HIGHLIGHT_MAX_FILE_BYTES = 512 * 1024;
const CSS_HIGHLIGHT_MAX_TOTAL_BYTES = 4 * 1024 * 1024;

function sanitizedCss(source) {
  let result = "";
  let index = 0;
  let state = "code";
  let quote = "";

  while (index < source.length) {
    const character = source[index];
    const next = source[index + 1];

    if (state === "comment") {
      if (character === "*" && next === "/") {
        result += "  ";
        index += 2;
        state = "code";
        continue;
      }
      result += character === "\n" || character === "\r" ? character : " ";
      index += 1;
      continue;
    }

    if (state === "string") {
      if (character === "\\" && next !== undefined) {
        result += " ";
        result += next === "\n" || next === "\r" ? next : " ";
        index += 2;
        continue;
      }
      if (character === quote) {
        result += " ";
        index += 1;
        state = "code";
        continue;
      }
      result += character === "\n" || character === "\r" ? character : " ";
      index += 1;
      continue;
    }

    if (character === "/" && next === "*") {
      result += "  ";
      index += 2;
      state = "comment";
      continue;
    }
    if (character === '"' || character === "'") {
      result += " ";
      index += 1;
      state = "string";
      quote = character;
      continue;
    }
    result += character;
    index += 1;
  }

  return result;
}

function lineAt(source, index) {
  let line = 1;
  for (let cursor = 0; cursor < index; cursor += 1) {
    if (source[cursor] === "\n") line += 1;
  }
  return line;
}

function topLevelParts(value, separator) {
  const parts = [];
  let depth = 0;
  let start = 0;

  for (let index = 0; index < value.length; index += 1) {
    const character = value[index];
    if (character === "(" || character === "[") depth += 1;
    if (character === ")" || character === "]") depth = Math.max(0, depth - 1);
    if (depth === 0 && separator(character)) {
      const part = value.slice(start, index).trim();
      if (part) parts.push(part);
      start = index + 1;
    }
  }

  const tail = value.slice(start).trim();
  if (tail) parts.push(tail);
  return parts;
}

function zeroBlurRingThickness(value) {
  const simpleLength = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[a-z%]+)?$/iu;
  const zeroLength = /^[+-]?(?:0+(?:\.0*)?|\.0+)(?:[a-z%]+)?$/iu;
  const thickSpread = /^\+?([23])(?:\.0+)?px$/iu;

  for (const component of topLevelParts(
    value,
    (character) => character === ",",
  )) {
    const lengths = topLevelParts(component, (character) =>
      /\s/u.test(character),
    ).filter((token) => simpleLength.test(token));
    if (
      lengths.length >= 4 &&
      zeroLength.test(lengths[0]) &&
      zeroLength.test(lengths[1]) &&
      zeroLength.test(lengths[2])
    ) {
      const thickness = lengths[3].match(thickSpread)?.[1];
      if (thickness) return thickness;
    }
  }

  return undefined;
}

function declarationError(source, start, end, forcedColors, relativePath) {
  if (forcedColors) return null;
  const declaration = source.slice(start, end).trim();
  const match = declaration.match(/^(outline|box-shadow)\s*:\s*([\s\S]*)$/iu);
  if (!match) return null;

  const [, property, value] = match;
  let thickness;
  let description;
  if (property.toLowerCase() === "outline") {
    thickness = value.match(/(?:^|[\s/,(])([23])px\b/iu)?.[1];
    description = "outline";
  } else {
    thickness = zeroBlurRingThickness(value);
    description = "zero-blur box-shadow ring";
  }
  if (!thickness) return null;

  const declarationOffset = source.slice(start, end).search(/\S/u);
  const line = lineAt(source, start + Math.max(0, declarationOffset));
  return `${relativePath}:${line} uses a ${thickness}px ${description} outside nested forced-colors.`;
}

export async function discoverSourceFilesWithCssBound(
  root,
  relativeDirectory = "src",
) {
  const files = [];
  const cssFiles = [];
  let exceededCssFileLimit = false;

  async function visit(directory) {
    const entries = await readdir(path.join(root, directory), {
      withFileTypes: true,
    });
    entries.sort((left, right) =>
      left.name < right.name ? -1 : left.name > right.name ? 1 : 0,
    );

    for (const entry of entries) {
      const relativePath = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        await visit(relativePath);
        if (exceededCssFileLimit) return;
        continue;
      }

      files.push(relativePath);
      if (!relativePath.endsWith(".css")) continue;
      cssFiles.push(relativePath);
      if (cssFiles.length > CSS_HIGHLIGHT_MAX_FILES) {
        exceededCssFileLimit = true;
        return;
      }
    }
  }

  await visit(relativeDirectory);
  return { cssFiles, exceededCssFileLimit, files };
}

export function highlightRingErrors(source, relativePath = "<css>") {
  const sanitized = sanitizedCss(source);
  const errors = [];
  const stack = [{ forcedColors: false, statementStart: 0 }];

  for (let index = 0; index < sanitized.length; index += 1) {
    const character = sanitized[index];
    const frame = stack.at(-1);
    if (!frame) break;

    if (character === "{") {
      const prelude = sanitized.slice(frame.statementStart, index).trim();
      const entersForcedColors =
        /^@media\b/iu.test(prelude) &&
        /\(\s*forced-colors\s*:\s*active\s*\)/iu.test(prelude) &&
        !/[,]|\b(?:not|or)\b/iu.test(prelude);
      stack.push({
        forcedColors: frame.forcedColors || entersForcedColors,
        statementStart: index + 1,
      });
      continue;
    }

    if (character === ";") {
      const error = declarationError(
        sanitized,
        frame.statementStart,
        index,
        frame.forcedColors,
        relativePath,
      );
      if (error) errors.push(error);
      frame.statementStart = index + 1;
      continue;
    }

    if (character === "}") {
      const error = declarationError(
        sanitized,
        frame.statementStart,
        index,
        frame.forcedColors,
        relativePath,
      );
      if (error) errors.push(error);
      stack.pop();
      const parent = stack.at(-1);
      if (parent) parent.statementStart = index + 1;
    }
  }

  return errors;
}

export async function cssHighlightPolicyErrors(root, relativeFiles) {
  if (relativeFiles.length > CSS_HIGHLIGHT_MAX_FILES) {
    return [
      `production CSS exceeds the ${CSS_HIGHLIGHT_MAX_FILES}-file scan limit (${relativeFiles.length} found).`,
    ];
  }

  const errors = [];
  let totalBytes = 0;
  for (const relativeFile of [...relativeFiles].sort()) {
    const absoluteFile = path.join(root, relativeFile);
    const metadata = await lstat(absoluteFile);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      errors.push(`${relativeFile} is not a regular production CSS file.`);
      continue;
    }
    if (metadata.size > CSS_HIGHLIGHT_MAX_FILE_BYTES) {
      errors.push(
        `${relativeFile} exceeds the ${CSS_HIGHLIGHT_MAX_FILE_BYTES}-byte scan limit (${metadata.size} bytes).`,
      );
      continue;
    }
    totalBytes += metadata.size;
    if (totalBytes > CSS_HIGHLIGHT_MAX_TOTAL_BYTES) {
      return [
        `production CSS exceeds the ${CSS_HIGHLIGHT_MAX_TOTAL_BYTES}-byte total scan limit.`,
      ];
    }
    errors.push(
      ...highlightRingErrors(
        await readFile(absoluteFile, "utf8"),
        relativeFile,
      ),
    );
  }
  return errors;
}
