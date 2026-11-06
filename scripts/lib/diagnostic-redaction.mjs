// Redaction for diagnostics that a lane prints, so a failure report never
// carries a local path. Known roots keep a readable label; any other absolute
// path is replaced through the end of its line or quoted field, because a
// path can hold spaces and nothing else marks where it ends.

const DEFAULT_LIMIT = 300;

/**
 * A local root under either separator, case-insensitively. Each separator is
 * matched once, not as a run, so a long run of slashes cannot make a match
 * attempt backtrack across it; a path with a doubled separator still falls to
 * the absolute path patterns below.
 */
function rootPattern(root) {
  const escaped = root
    .replace(/[\\/]+$/u, "")
    .split(/[\\/]+/u)
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&"))
    .join("[\\\\/]");
  return new RegExp(`${escaped}(?=$|[\\\\/\\s"'\`:])`, "giu");
}

// The rest of a path: anything up to a line end, a double quote or a
// backtick. Spaces, parentheses and apostrophes can all be part of a path.
const REST = String.raw`[^\r\n"\x60]*`;
// Where a bare path may start: not inside a word, a URL or another path.
const BOUNDARY = String.raw`(^|[\s"'\x60(\[=,;<])`;
const ABSOLUTE_PATHS = [
  // file: URLs, with or without an authority.
  [new RegExp(String.raw`\bfile:${REST}`, "giu"), "file:<path>"],
  // Vite serves files outside the root at /@fs/<absolute path>.
  [new RegExp(String.raw`/@fs/${REST}`, "gu"), "/@fs/<path>"],
  // UNC and device paths: \\server\share, \\?\C:\, //server/share.
  [
    new RegExp(
      String.raw`${BOUNDARY}(?:\\\\|//)[^\s\\/"\x60]+[\\/]${REST}`,
      "gmu",
    ),
    "$1<path>",
  ],
  // Drive paths: C:\..., c:/....
  [new RegExp(String.raw`(?<![\w/\\])[A-Za-z]:[\\/]${REST}`, "gu"), "<path>"],
  // POSIX paths of at least two segments, so "/" and "and/or" stay. A
  // segment, the first included, can hold spaces. The lookahead asks for a
  // non-space in the first segment without a second repetition over it, so
  // a match attempt stays linear in the input.
  [
    new RegExp(
      String.raw`${BOUNDARY}/(?=[ 	]*[^\s/"\x60])[^/\r\n"\x60]*/${REST}`,
      "gmu",
    ),
    "$1<path>",
  ],
];

/**
 * Builds a sanitizer for the given local roots: [absolute root, label]
 * pairs, longest first so a nested root keeps its own label.
 */
export function createDiagnosticSanitizer(roots) {
  const patterns = roots
    .filter(([root]) => typeof root === "string" && root.length > 1)
    .sort(([left], [right]) => right.length - left.length)
    .map(([root, label]) => [rootPattern(root), label]);
  return function sanitize(text, limit = DEFAULT_LIMIT) {
    let clean = String(text);
    for (const [pattern, label] of patterns)
      clean = clean.replace(pattern, label);
    for (const [pattern, replacement] of ABSOLUTE_PATHS)
      clean = clean.replace(pattern, replacement);
    return clean.length > limit ? `${clean.slice(0, limit)}…` : clean;
  };
}

/**
 * The redacted end of an output tail, at most limit characters of whole
 * lines. Text is redacted before it is cut, so a cut never strips a path's
 * absolute prefix. A leading line that was cut, here or by whoever kept the
 * tail (partial), is dropped.
 */
export function redactedTail(sanitize, text, limit, { partial = false } = {}) {
  const clean = sanitize(text, Number.POSITIVE_INFINITY);
  const cut = clean.length > limit;
  let tail = cut ? clean.slice(-limit) : clean;
  if (cut || partial) {
    const lineEnd = tail.indexOf("\n");
    tail = lineEnd === -1 ? "" : tail.slice(lineEnd + 1);
  }
  return tail.trim();
}
