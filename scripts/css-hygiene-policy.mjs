import { lstat, readFile } from "node:fs/promises";
import path from "node:path";

import postcss from "postcss";

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

/** Splits `text` on `delimiter` where it sits outside brackets and strings. */
function splitTopLevel(text, delimiter) {
  const parts = [];
  let depth = 0;
  let quote = "";
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (quote) {
      if (character === "\\") index += 1;
      else if (character === quote) quote = "";
    } else if (character === '"' || character === "'") {
      quote = character;
    } else if (character === "(" || character === "[") {
      depth += 1;
    } else if (character === ")" || character === "]") {
      depth = Math.max(0, depth - 1);
    } else if (depth === 0 && delimiter.test(text.slice(index))) {
      parts.push({ start, text: text.slice(start, index) });
      start = index + text.slice(index).match(delimiter)[0].length;
      index = start - 1;
    }
  }
  parts.push({ start, text: text.slice(start) });
  return parts;
}

function normalizeSelector(selector) {
  let result = "";
  let quote = "";
  let pendingSpace = false;
  for (let index = 0; index < selector.length; index += 1) {
    const character = selector[index];
    if (quote) {
      result += character;
      if (character === "\\" && index + 1 < selector.length) {
        result += selector[index + 1];
        index += 1;
      } else if (character === quote) {
        quote = "";
      }
    } else if (/\s/u.test(character)) {
      pendingSpace = true;
    } else {
      if (pendingSpace && result) result += " ";
      pendingSpace = false;
      if (character === '"' || character === "'") quote = character;
      result += character;
    }
  }
  return result.trim();
}

/** One element owns a surface's radius. A per-corner longhand may only square
 * a corner where segments of a control meet (a split button). A box rounded
 * on some corners is a slice of a larger surface, and that surface should own
 * the radius instead of each slice repeating it. */
const CORNER_RADIUS =
  /^border-(?:(?:top|bottom)-(?:left|right)|(?:start|end)-(?:start|end))-radius$/u;

/** `!important` only restates user and system accessibility preferences. */
const ACCESSIBILITY_FEATURES = new Set([
  "prefers-reduced-motion:reduce",
  "forced-colors:active",
]);

/** True when the outer parentheses enclose the whole condition, as in
 * `((forced-colors: active))` but not `(a) and (b)`. */
function wrappedInParentheses(condition) {
  if (!condition.startsWith("(") || !condition.endsWith(")")) return false;
  let depth = 0;
  for (let index = 0; index < condition.length; index += 1) {
    if (condition[index] === "(") depth += 1;
    else if (condition[index] === ")") depth -= 1;
    if (depth === 0 && index < condition.length - 1) return false;
  }
  return true;
}

function mediaFeature(condition) {
  let feature = condition.trim();
  while (wrappedInParentheses(feature)) feature = feature.slice(1, -1).trim();
  return feature.replace(/\s+/gu, "").toLowerCase();
}

/** True when every query in the list requires an accessibility preference:
 * each alternative must state it as a top-level `and` conjunct, not under
 * `not` or beside an `or`. */
function accessibilityMedia(params) {
  return splitTopLevel(params, /^,/u).every(({ text }) => {
    const query = text.trim();
    if (!query || /^not\b/iu.test(query)) return false;
    if (splitTopLevel(query, /^\s+or\s+/iu).length > 1) return false;
    return splitTopLevel(query, /^\s+and\s+/iu).some(({ text: conjunct }) =>
      ACCESSIBILITY_FEATURES.has(mediaFeature(conjunct)),
    );
  });
}

const Z_INDEX_LAYER =
  /^(?:auto|inherit|initial|unset|revert|var\(\s*--z-[a-z0-9-]+\s*\)|calc\(\s*var\(\s*--z-[a-z0-9-]+\s*\)\s*[+-]\s*\d+\s*\))$/iu;

const QUOTED = /"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'/gu;

function declarationRecords(declaration, relativePath) {
  const property = declaration.prop.toLowerCase();
  const value = declaration.value.trim().replace(/\s+/gu, " ");
  const unquoted = value.replace(QUOTED, (literal) =>
    " ".repeat(literal.length),
  );
  const line = declaration.source.start.line;
  const records = {
    cornerRadii: [],
    importants: [],
    rawColors: [],
    rawRadii: [],
    rawZIndexes: [],
  };

  if (
    CORNER_RADIUS.test(property) &&
    !/^var\(\s*--radius-none\s*\)$/iu.test(value)
  ) {
    records.cornerRadii.push({ line, property, relativePath, value });
  }

  if (declaration.important && !importantAllowed(declaration)) {
    records.importants.push({ line, property, relativePath });
  }

  if (
    relativePath !== GLOBAL_TOKEN_AUTHORITY &&
    property === "border-radius" &&
    !/^var\(\s*--radius-[a-z0-9-]+\s*\)$/iu.test(value)
  ) {
    records.rawRadii.push({ line, relativePath, value });
  }

  if (property === "z-index" && !Z_INDEX_LAYER.test(value)) {
    records.rawZIndexes.push({ line, relativePath, value });
  }

  const ownsTokens =
    property.startsWith("--") && FEATURE_TOKEN_OWNERS.has(relativePath);
  if (relativePath !== GLOBAL_TOKEN_AUTHORITY && !ownsTokens) {
    const colorPattern =
      /#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{4}|[0-9a-f]{3})\b|\brgba?\s*\(/giu;
    for (const color of unquoted.matchAll(colorPattern)) {
      const literal = color[0].startsWith("#")
        ? color[0]
        : color[0].toLowerCase().replace(/\s+/gu, "");
      records.rawColors.push({ line, literal, relativePath });
    }
  }
  return records;
}

function importantAllowed(node) {
  for (let parent = node.parent; parent; parent = parent.parent) {
    if (
      parent.type === "atrule" &&
      parent.name.toLowerCase() === "media" &&
      accessibilityMedia(parent.params)
    ) {
      return true;
    }
  }
  return false;
}

function insideKeyframes(node) {
  for (let parent = node.parent; parent; parent = parent.parent) {
    if (parent.type === "atrule" && /keyframes$/iu.test(parent.name)) {
      return true;
    }
  }
  return false;
}

const PARSE_REASONS = new Map([
  ["Unclosed block", "has an unclosed block"],
  ["Unclosed bracket", "has an unclosed bracket"],
  ["Unclosed comment", "has an unterminated comment"],
  ["Unclosed quote", "has an unterminated string"],
  ["Unclosed string", "has an unterminated string"],
  ["Unexpected }", 'has an unexpected "}"'],
]);

function parseError(error, relativePath) {
  const reason =
    PARSE_REASONS.get(error.reason) ?? `is not valid CSS (${error.reason})`;
  return `${relativePath}:${error.line ?? 1} ${reason}.`;
}

function selectorRecords(rule, relativePath) {
  const selector = rule.raws.selector?.raw ?? rule.selector;
  const startLine = rule.source.start.line;
  return splitTopLevel(selector, /^,/u)
    .map(({ start, text }) => {
      const leading = text.length - text.trimStart().length;
      const before = selector.slice(0, start + leading);
      return {
        line: startLine + (before.match(/\n/gu)?.length ?? 0),
        relativePath,
        selector: normalizeSelector(text.replace(/\/\*[\s\S]*?\*\//gu, " ")),
      };
    })
    .filter(({ selector: part }) => part.length > 0);
}

function analyzeCss(source, relativePath) {
  const result = {
    cornerRadii: [],
    importants: [],
    parseErrors: [],
    rawColors: [],
    rawRadii: [],
    rawZIndexes: [],
    selectors: [],
  };
  let root;
  try {
    root = postcss.parse(source, { from: undefined });
  } catch (error) {
    if (error?.name !== "CssSyntaxError") throw error;
    result.parseErrors.push(parseError(error, relativePath));
    return result;
  }
  root.walkRules((rule) => {
    if (!insideKeyframes(rule)) {
      result.selectors.push(...selectorRecords(rule, relativePath));
    }
  });
  root.walkDecls((declaration) => {
    const records = declarationRecords(declaration, relativePath);
    for (const [kind, entries] of Object.entries(records)) {
      result[kind].push(...entries);
    }
  });
  return result;
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
  const cornerRadii = [];
  const importants = [];
  for (const file of [...files].sort((left, right) =>
    compareText(left.relativePath, right.relativePath),
  )) {
    const result = analyzeCss(file.source, file.relativePath);
    parseErrors.push(...result.parseErrors);
    rawColors.push(...result.rawColors);
    rawRadii.push(...result.rawRadii);
    rawZIndexes.push(...result.rawZIndexes);
    cornerRadii.push(...result.cornerRadii);
    importants.push(...result.importants);
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
  cornerRadii.sort(compareLocation);
  importants.sort(compareLocation);
  return {
    cornerRadii,
    duplicateSelectors,
    importants,
    parseErrors,
    rawColors,
    rawRadii,
    rawZIndexes,
  };
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
    ...inventory.cornerRadii.map(
      ({ line, property, relativePath, value }) =>
        `${relativePath}:${line} sets "${property}: ${value}"; a corner may only be squared with var(--radius-none), so give the whole surface one border-radius.`,
    ),
    ...inventory.importants.map(
      ({ line, property, relativePath }) =>
        `${relativePath}:${line} marks "${property}" !important; fix the cascade (a variant, token, or the owning rule) instead.`,
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
