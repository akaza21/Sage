// Redaction for diagnostics that a lane prints, so a failure report never
// carries a local path. Known roots keep a readable label; any other absolute
// path is replaced through the end of its line or quoted field, because a
// path can hold spaces and nothing else marks where it ends.

const DEFAULT_LIMIT = 300;

/** A local root under either separator, case-insensitively. */
function rootPattern(root) {
  const escaped = root
    .replace(/[\\/]+$/u, "")
    .split(/[\\/]+/u)
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&"))
    .join("[\\\\/]+");
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
  // POSIX paths of at least two segments, so "/" and "and/or" stay.
  [new RegExp(String.raw`${BOUNDARY}/[^\s/"\x60]+/${REST}`, "gmu"), "$1<path>"],
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
