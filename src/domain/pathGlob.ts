import { utf8ByteLength } from "./text";

/**
 * Sage's path glob dialect, used by Search's include and exclude fields and
 * by `files.exclude`. A pattern matches a whole Project-relative path with `/`
 * separators: a `**` segment matches any number of segments, `*` any run of
 * characters within a segment and `?` one character. Matching is
 * case-sensitive. Mirrors src-tauri/src/workspace/path_glob.rs; both test
 * against path_glob/cases.json. The matcher lives in `pathGlobMatcher.ts`,
 * which Explorer loads on its own so it stays out of the startup bundle.
 */

export const MAX_PATH_GLOB_BYTES = 256;

/**
 * Relative and bounded: no leading `/` or `~`, backslash, colon, control
 * character or lone surrogate (Rust would refuse the save), and no empty,
 * `.` or `..` segment.
 */
export function validPathGlob(value: unknown): value is string {
  return (
    typeof value === "string" &&
    utf8ByteLength(value) <= MAX_PATH_GLOB_BYTES &&
    !/^[/~]|[\\:\p{Cc}\uD800-\uDFFF]|(?:^|\/)\.{0,2}(?:\/|$)/u.test(value)
  );
}
