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

/** Reads a media query list in one forward pass. Each query becomes its
 * top-level words and parenthesized groups; whitespace runs separate them.
 * The prelude is already masked, so strings and comments hold no brackets. */
function mediaQueryList(text) {
  const queries = [[]];
  let depth = 0;
  let token = "";
  const flush = () => {
    if (token) queries.at(-1).push(token);
    token = "";
  };
  for (const character of text) {
    if (depth > 0) {
      token += character;
      if (character === "(") depth += 1;
      else if (character === ")") {
        depth -= 1;
        if (depth === 0) flush();
      }
    } else if (character === "(") {
      flush();
      token = character;
      depth = 1;
    } else if (character === ",") {
      flush();
      queries.push([]);
    } else if (/\s/u.test(character)) {
      flush();
    } else {
      token += character;
    }
  }
  flush();
  return depth === 0 ? queries : [];
}

/** The feature inside a group with redundant parentheses and whitespace
 * removed: `( ( forced-colors : active ) )` reads `forced-colors:active`. */
function mediaFeature(group) {
  const condition = group.replace(/\s+/gu, "");
  const closing = new Array(condition.length);
  const open = [];
  for (let index = 0; index < condition.length; index += 1) {
    if (condition[index] === "(") open.push(index);
    else if (condition[index] === ")") closing[open.pop()] = index;
  }
  let start = 0;
  let end = condition.length - 1;
  while (start < end && closing[start] === end) {
    start += 1;
    end -= 1;
  }
  return condition.slice(start, end + 1).toLowerCase();
}

/** True for `@media` whose every query requires an accessibility preference
 * as a top-level `and` condition, never after `not` or beside an `or`. */
function accessibilityMedia(prelude) {
  const media = prelude.match(/^@media\b/iu);
  if (!media) return false;
  const queries = mediaQueryList(prelude.slice(media[0].length));
  return (
    queries.length > 0 &&
    queries.every((tokens) => {
      const words = tokens.map((token) => token.toLowerCase());
      if (words.length === 0 || words[0] === "not" || words.includes("or")) {
        return false;
      }
      // A `not` negates the condition right after it, so that condition
      // cannot be the preference the query requires.
      return tokens.some(
        (token, index) =>
          token.startsWith("(") &&
          words[index - 1] !== "not" &&
          ACCESSIBILITY_FEATURES.has(mediaFeature(token)),
      );
    })
  );
}

/** A conditional group nested in a style rule holds that rule's
 * declarations (CSS nesting), so its body is read as declarations. */
const CONDITIONAL_GROUP =
  /^@(?:media|supports|container|layer|scope|starting-style)\b/iu;

const VENDOR_PREFIX = /^-(?:webkit|moz|ms|o)-/u;

const Z_INDEX_LAYER =
  /^(?:auto|inherit|initial|unset|revert|var\(\s*--z-[a-z0-9-]+\s*\)|calc\(\s*var\(\s*--z-[a-z0-9-]+\s*\)\s*[+-]\s*\d+\s*\))$/iu;

function declarationRecords(
  source,
  masked,
  start,
  end,
  relativePath,
  lineAt,
  importantAllowed,
) {
  const range = trimmedRange(masked, start, end);
  const none = {
    cornerRadii: [],
    importants: [],
    rawColors: [],
    rawRadii: [],
    rawZIndexes: [],
  };
  if (range.start >= range.end) return none;
  const declaration = masked.slice(range.start, range.end);
  const match = declaration.match(/^(-{0,2}[a-z][a-z0-9-]*)\s*:/iu);
  if (!match) return none;

  // Vendor aliases follow the rules of the standard property they stand for.
  const declared = match[1].toLowerCase();
  const property = declared.replace(VENDOR_PREFIX, "");
  const colonOffset = declaration.indexOf(":");
  const valueStart = range.start + colonOffset + 1;
  const valueRange = trimmedRange(masked, valueStart, range.end);
  // The priority is not part of the value: read it, then check the value.
  const maskedDeclarationValue = masked.slice(valueRange.start, valueRange.end);
  const priority = maskedDeclarationValue.match(/\s*!\s*important\s*$/iu);
  const valueEnd = valueRange.end - (priority?.[0].length ?? 0);
  const maskedValue = masked.slice(valueRange.start, valueEnd);
  const rawValue = source
    .slice(valueRange.start, valueEnd)
    .trim()
    .replace(/\s+/gu, " ");
  const line = lineAt(range.start);
  const rawRadii = [];
  const rawColors = [];
  const rawZIndexes = [];
  const cornerRadii = [];
  const importants = [];

  if (
    CORNER_RADIUS.test(property) &&
    !/^var\(\s*--radius-none\s*\)$/iu.test(maskedValue.trim())
  ) {
    cornerRadii.push({
      line,
      property: declared,
      relativePath,
      value: rawValue,
    });
  }

  if (!importantAllowed && priority) {
    importants.push({ line, property: declared, relativePath });
  }

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
  return { cornerRadii, importants, rawColors, rawRadii, rawZIndexes };
}

function analyzeCss(source, relativePath) {
  const lineAt = lineIndex(source);
  const { errors, masked } = maskCss(source, relativePath, lineAt);
  const selectors = [];
  const rawColors = [];
  const rawRadii = [];
  const rawZIndexes = [];
  const cornerRadii = [];
  const importants = [];
  const stack = [
    {
      importantAllowed: false,
      kind: "rules",
      openIndex: -1,
      statementStart: 0,
    },
  ];
  const collect = (frame, end) => {
    const records = declarationRecords(
      source,
      masked,
      frame.statementStart,
      end,
      relativePath,
      lineAt,
      frame.importantAllowed,
    );
    cornerRadii.push(...records.cornerRadii);
    importants.push(...records.importants);
    rawColors.push(...records.rawColors);
    rawRadii.push(...records.rawRadii);
    rawZIndexes.push(...records.rawZIndexes);
  };

  for (let index = 0; index < masked.length; index += 1) {
    const frame = stack.at(-1);
    const character = masked[index];
    if (character === "{") {
      const range = trimmedRange(masked, frame.statementStart, index);
      const prelude = masked.slice(range.start, range.end);
      let kind;
      if (frame.kind === "keyframes") kind = "declarations";
      else if (frame.kind === "declarations" && CONDITIONAL_GROUP.test(prelude))
        kind = "declarations";
      else if (prelude.startsWith("@")) kind = atRuleBlockKind(prelude);
      else {
        kind = "declarations";
        selectors.push(
          ...selectorParts(source, masked, range.start, range.end, lineAt).map(
            (selector) => ({ ...selector, relativePath }),
          ),
        );
      }
      stack.push({
        importantAllowed: frame.importantAllowed || accessibilityMedia(prelude),
        kind,
        openIndex: index,
        statementStart: index + 1,
      });
    } else if (character === ";") {
      if (frame.kind === "declarations") collect(frame, index);
      frame.statementStart = index + 1;
    } else if (character === "}") {
      if (stack.length === 1) {
        errors.push(`${relativePath}:${lineAt(index)} has an unexpected "}".`);
        frame.statementStart = index + 1;
        continue;
      }
      if (frame.kind === "declarations") collect(frame, index);
      stack.pop();
      stack.at(-1).statementStart = index + 1;
    }
  }

  if (stack.length > 1) {
    errors.push(
      `${relativePath}:${lineAt(stack[1].openIndex)} has an unclosed block.`,
    );
  }
  return {
    cornerRadii,
    importants,
    parseErrors: errors,
    rawColors,
    rawRadii,
    rawZIndexes,
    selectors,
  };
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
