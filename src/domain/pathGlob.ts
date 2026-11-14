import { utf8ByteLength } from "./text";

/**
 * Matches whole project-relative / paths; a whole ** segment matches zero or
 * more segments; * and ? match within a segment; case-sensitive. Mirrors src-tauri/src/workspace/path_glob.rs; both test
 * against path_glob/cases.json.
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
