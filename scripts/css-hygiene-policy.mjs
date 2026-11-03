import { lstat, readFile } from "node:fs/promises";
import path from "node:path";

export const CSS_HYGIENE_MAX_FILES = 128;
export const CSS_HYGIENE_MAX_FILE_BYTES = 512 * 1024;
const CSS_HYGIENE_MAX_TOTAL_BYTES = 4 * 1024 * 1024;
const GLOBAL_TOKEN_AUTHORITY = "src/styles/global.css";
// Stylesheets that may define raw colours, as custom properties only, beside
// the global authority. A lazy feature keeps tokens no other code uses in its
// own chunk so they stay off the initial CSS; its rules still read them
// through var().
const FEATURE_TOKEN_OWNERS = new Map([
  [
    "src/features/browser/browser-device.css",
    "Device toolbar gutter, grip and field fills; loads with the lazy toolbar.",
  ],
  [
    "src/styles/workbench-chrome.css",
    "Title bar fill, defined beside the chrome rules before this check.",
  ],
]);

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function lineIndex(source) {
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "\n") starts.push(index + 1);
  }
  return (offset) => {
    let low = 0;
    let high = starts.length;
    while (low + 1 < high) {
      const middle = Math.floor((low + high) / 2);
      if (starts[middle] <= offset) low = middle;
      else high = middle;
    }
    return low + 1;
  };
}

function maskCss(source, relativePath, lineAt) {
  const characters = [...source];
  const errors = [];
  let state = "code";
  let quote = "";
  let constructStart = 0;

  for (let index = 0; index < characters.length; index += 1) {
    const character = characters[index];
    const next = characters[index + 1];
    if (state === "comment") {
      if (character === "*" && next === "/") {
        characters[index] = " ";
        characters[index + 1] = " ";
        index += 1;
        state = "code";
      } else if (character !== "\n" && character !== "\r") {
        characters[index] = " ";
      }
      continue;
    }
    if (state === "string") {
      if (character === "\\" && next !== undefined) {
        characters[index] = " ";
        if (next !== "\n" && next !== "\r") characters[index + 1] = " ";
        index += 1;
      } else if (character === quote) {
        characters[index] = " ";
        state = "code";
      } else if (character !== "\n" && character !== "\r") {
        characters[index] = " ";
      }
      continue;
    }
    if (character === "/" && next === "*") {
      constructStart = index;
      characters[index] = " ";
      characters[index + 1] = " ";
      index += 1;
      state = "comment";
    } else if (character === '"' || character === "'") {
      constructStart = index;
      quote = character;
      characters[index] = " ";
      state = "string";
    }
  }

  if (state !== "code") {
    errors.push(
      `${relativePath}:${lineAt(constructStart)} has an unterminated ${state}.`,
    );
  }
  return { errors, masked: characters.join("") };
}

function trimmedRange(value, start, end) {
  while (start < end && /\s/u.test(value[start])) start += 1;
  while (end > start && /\s/u.test(value[end - 1])) end -= 1;
  return { end, start };
}

function normalizeSelector(selector) {
  let result = "";
  let state = "code";
  let quote = "";
  let pendingSpace = false;
  for (let index = 0; index < selector.length; index += 1) {
    const character = selector[index];
    const next = selector[index + 1];
    if (state === "comment") {
      if (character === "*" && next === "/") {
        index += 1;
        state = "code";
        pendingSpace = true;
      }
      continue;
    }
    if (state === "string") {
      result += character;
      if (character === "\\" && next !== undefined) {
        result += next;
        index += 1;
      } else if (character === quote) {
        state = "code";
      }
      continue;
    }
    if (character === "/" && next === "*") {
      index += 1;
      state = "comment";
    } else if (character === '"' || character === "'") {
      if (pendingSpace && result) result += " ";
      pendingSpace = false;
      quote = character;
      state = "string";
      result += character;
    } else if (/\s/u.test(character)) {
      pendingSpace = true;
    } else {
      if (pendingSpace && result) result += " ";
      pendingSpace = false;
      result += character;
    }
  }
  return result.trim();
}

function selectorParts(source, masked, start, end, lineAt) {
  const parts = [];
  let partStart = start;
  let roundDepth = 0;
  let squareDepth = 0;
  for (let index = start; index < end; index += 1) {
    if (masked[index] === "(") roundDepth += 1;
    else if (masked[index] === ")") roundDepth = Math.max(0, roundDepth - 1);
    else if (masked[index] === "[") squareDepth += 1;
    else if (masked[index] === "]") squareDepth = Math.max(0, squareDepth - 1);
    else if (masked[index] === "," && roundDepth === 0 && squareDepth === 0) {
      const range = trimmedRange(masked, partStart, index);
      if (range.start < range.end) {
        parts.push({
          line: lineAt(range.start),
          selector: normalizeSelector(source.slice(range.start, range.end)),
        });
      }
      partStart = index + 1;
    }
  }
  const range = trimmedRange(masked, partStart, end);
  if (range.start < range.end) {
    parts.push({
      line: lineAt(range.start),
      selector: normalizeSelector(source.slice(range.start, range.end)),
    });
  }
  return parts;
}

function atRuleBlockKind(prelude) {
  if (/^@(?:-[a-z]+-)?keyframes\b/iu.test(prelude)) return "keyframes";
  if (
    /^@(?:media|supports|layer|container|scope|document|starting-style)\b/iu.test(
      prelude,
    )
  ) {
    return "rules";
  }
  return "declarations";
}

/** A z-index names a stacking layer from global.css, optionally offset by a
 * whole step inside `calc()` when a third-party layer has to be cleared. */
const Z_INDEX_LAYER =
  /^(?:auto|inherit|initial|unset|revert|var\(\s*--z-[a-z0-9-]+\s*\)|calc\(\s*var\(\s*--z-[a-z0-9-]+\s*\)\s*[+-]\s*\d+\s*\))$/iu;

function declarationRecords(source, masked, start, end, relativePath, lineAt) {
  const range = trimmedRange(masked, start, end);
  const none = { rawColors: [], rawRadii: [], rawZIndexes: [] };
  if (range.start >= range.end) return none;
  const declaration = masked.slice(range.start, range.end);
  const match = declaration.match(/^((?:--)?[a-z][a-z0-9-]*)\s*:/iu);
  if (!match) return none;

  const property = match[1].toLowerCase();
  const colonOffset = declaration.indexOf(":");
  const valueStart = range.start + colonOffset + 1;
  const valueRange = trimmedRange(masked, valueStart, range.end);
  const maskedValue = masked.slice(valueRange.start, valueRange.end);
  const rawValue = source
    .slice(valueRange.start, valueRange.end)
    .trim()
    .replace(/\s+/gu, " ");
  const line = lineAt(range.start);
  const rawRadii = [];
  const rawColors = [];
  const rawZIndexes = [];

  if (
    relativePath !== GLOBAL_TOKEN_AUTHORITY &&
    property === "border-radius" &&
    !/^var\(\s*--radius-[a-z0-9-]+\s*\)$/iu.test(maskedValue.trim())
  ) {
    rawRadii.push({ line, relativePath, value: rawValue });
  }

  if (property === "z-index" && !Z_INDEX_LAYER.test(maskedValue.trim())) {
    rawZIndexes.push({ line, relativePath, value: rawValue });
  }

  const ownsTokens =
    property.startsWith("--") && FEATURE_TOKEN_OWNERS.has(relativePath);
  if (relativePath !== GLOBAL_TOKEN_AUTHORITY && !ownsTokens) {
    const colorPattern =
      /#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{4}|[0-9a-f]{3})\b|\brgba?\s*\(/giu;
    for (const color of maskedValue.matchAll(colorPattern)) {
      const literal = color[0].startsWith("#")
        ? color[0]
        : color[0].toLowerCase().replace(/\s+/gu, "");
      rawColors.push({ line, literal, relativePath });
    }
  }
  return { rawColors, rawRadii, rawZIndexes };
}

function analyzeCss(source, relativePath) {
  const lineAt = lineIndex(source);
  const { errors, masked } = maskCss(source, relativePath, lineAt);
  const selectors = [];
  const rawColors = [];
  const rawRadii = [];
  const rawZIndexes = [];
  const stack = [{ kind: "rules", openIndex: -1, statementStart: 0 }];

  for (let index = 0; index < masked.length; index += 1) {
    const frame = stack.at(-1);
    const character = masked[index];
    if (character === "{") {
      const range = trimmedRange(masked, frame.statementStart, index);
      const prelude = masked.slice(range.start, range.end);
      let kind;
      if (frame.kind === "keyframes") kind = "declarations";
      else if (prelude.startsWith("@")) kind = atRuleBlockKind(prelude);
      else {
        kind = "declarations";
        selectors.push(
          ...selectorParts(source, masked, range.start, range.end, lineAt).map(
            (selector) => ({ ...selector, relativePath }),
          ),
        );
      }
      stack.push({ kind, openIndex: index, statementStart: index + 1 });
    } else if (character === ";") {
      if (frame.kind === "declarations") {
        const records = declarationRecords(
          source,
          masked,
          frame.statementStart,
          index,
          relativePath,
          lineAt,
        );
        rawColors.push(...records.rawColors);
        rawRadii.push(...records.rawRadii);
        rawZIndexes.push(...records.rawZIndexes);
      }
      frame.statementStart = index + 1;
    } else if (character === "}") {
      if (stack.length === 1) {
        errors.push(`${relativePath}:${lineAt(index)} has an unexpected "}".`);
        frame.statementStart = index + 1;
        continue;
      }
      if (frame.kind === "declarations") {
        const records = declarationRecords(
          source,
          masked,
          frame.statementStart,
          index,
          relativePath,
          lineAt,
        );
        rawColors.push(...records.rawColors);
        rawRadii.push(...records.rawRadii);
        rawZIndexes.push(...records.rawZIndexes);
      }
      stack.pop();
      stack.at(-1).statementStart = index + 1;
    }
  }

  if (stack.length > 1) {
    errors.push(
      `${relativePath}:${lineAt(stack[1].openIndex)} has an unclosed block.`,
    );
  }
  return { parseErrors: errors, rawColors, rawRadii, rawZIndexes, selectors };
}

function compareLocation(left, right) {
  return (
    compareText(left.relativePath, right.relativePath) || left.line - right.line
  );
}

export function analyzeCssInventory(files) {
  const ownership = new Map();
  const parseErrors = [];
  const rawColors = [];
  const rawRadii = [];
  const rawZIndexes = [];
  for (const file of [...files].sort((left, right) =>
    compareText(left.relativePath, right.relativePath),
  )) {
    const result = analyzeCss(file.source, file.relativePath);
    parseErrors.push(...result.parseErrors);
    rawColors.push(...result.rawColors);
    rawRadii.push(...result.rawRadii);
    rawZIndexes.push(...result.rawZIndexes);
    for (const occurrence of result.selectors) {
      const owners = ownership.get(occurrence.selector) ?? new Map();
      const current = owners.get(occurrence.relativePath);
      if (current === undefined || occurrence.line < current.line) {
        owners.set(occurrence.relativePath, {
          line: occurrence.line,
          relativePath: occurrence.relativePath,
        });
      }
      ownership.set(occurrence.selector, owners);
    }
  }

  const duplicateSelectors = [...ownership.entries()]
    .filter(([, owners]) => owners.size > 1)
    .map(([selector, owners]) => ({
      selector,
      owners: [...owners.values()].sort(compareLocation),
    }))
    .sort((left, right) => compareText(left.selector, right.selector));
  parseErrors.sort(compareText);
  rawColors.sort(
    (left, right) =>
      compareLocation(left, right) || compareText(left.literal, right.literal),
  );
  rawRadii.sort(
    (left, right) =>
      compareLocation(left, right) || compareText(left.value, right.value),
  );
  rawZIndexes.sort(
    (left, right) =>
      compareLocation(left, right) || compareText(left.value, right.value),
  );
  return { duplicateSelectors, parseErrors, rawColors, rawRadii, rawZIndexes };
}

function inventoryErrors(inventory) {
  return [
    ...inventory.parseErrors,
    ...inventory.duplicateSelectors.map(
      ({ owners, selector }) =>
        `selector "${selector}" is owned by ${owners
          .map(({ line, relativePath }) => `${relativePath}:${line}`)
          .join(", ")}.`,
    ),
    ...inventory.rawRadii.map(
      ({ line, relativePath, value }) =>
        `${relativePath}:${line} uses raw border-radius "${value}".`,
    ),
    ...inventory.rawZIndexes.map(
      ({ line, relativePath, value }) =>
        `${relativePath}:${line} uses raw z-index "${value}"; use a --z-* layer.`,
    ),
    ...inventory.rawColors.map(
      ({ line, literal, relativePath }) =>
        `${relativePath}:${line} uses raw color literal "${literal}".`,
    ),
  ];
}

export async function cssHygienePolicyErrors(root, relativeFiles) {
  if (relativeFiles.length > CSS_HYGIENE_MAX_FILES) {
    return [
      `production CSS exceeds the ${CSS_HYGIENE_MAX_FILES}-file scan limit (${relativeFiles.length} found).`,
    ];
  }

  const files = [];
  let totalBytes = 0;
  for (const relativePath of [...relativeFiles].sort(compareText)) {
    const absolutePath = path.join(root, relativePath);
    const metadata = await lstat(absolutePath);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      return [`${relativePath} is not a regular production CSS file.`];
    }
    if (metadata.size > CSS_HYGIENE_MAX_FILE_BYTES) {
      return [
        `${relativePath} exceeds the ${CSS_HYGIENE_MAX_FILE_BYTES}-byte scan limit (${metadata.size} bytes).`,
      ];
    }
    totalBytes += metadata.size;
    if (totalBytes > CSS_HYGIENE_MAX_TOTAL_BYTES) {
      return [
        `production CSS exceeds the ${CSS_HYGIENE_MAX_TOTAL_BYTES}-byte hygiene scan limit.`,
      ];
    }
    files.push({
      relativePath: relativePath.split(path.sep).join("/"),
      source: await readFile(absolutePath, "utf8"),
    });
  }
  return inventoryErrors(analyzeCssInventory(files));
}
