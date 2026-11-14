import { validPathGlob } from "./pathGlob";

export type PathGlobMatcher = (path: string) => boolean;

/** `*` and `?` within one name, with one backtrack point: linear per retry, never exponential. */
function wildcard(pattern: string[], value: string[]): boolean {
  let at = 0;
  let index = 0;
  let star = -1;
  let retry = 0;
  while (index < value.length) {
    if (pattern[at] === "*") {
      star = at++;
      retry = index;
    } else if (pattern[at] === "?" || pattern[at] === value[index]) {
      at += 1;
      index += 1;
    } else if (star < 0) {
      return false;
    } else {
      at = star + 1;
      index = ++retry;
    }
  }
  while (pattern[at] === "*") at += 1;
  return at === pattern.length;
}

type Segments = (string[] | null)[];

function segmentsMatch(pattern: Segments, path: string[][]): boolean {
  let row = path.map(() => false);
  row.unshift(true);
  for (const segment of pattern) {
    const next = [segment === null && row[0]];
    for (let index = 1; index < row.length; index += 1) {
      next[index] =
        segment === null
          ? row[index] || next[index - 1]
          : row[index - 1] && wildcard(segment, path[index - 1]);
    }
    row = next;
  }
  return row[path.length];
}

const split = (text: string, glob: boolean) =>
  text.split("/").map((part) => (glob && part === "**" ? null : [...part]));

const compiled = new WeakMap<readonly string[], PathGlobMatcher>();

export function pathGlobMatcher(patterns: readonly string[]): PathGlobMatcher {
  let matcher = compiled.get(patterns);
  if (!matcher) {
    const lists = patterns.filter(validPathGlob).map((p) => split(p, true));
    matcher = (path) => {
      if (lists.length === 0) return false;
      const parts = split(path, false) as string[][];
      return lists.some((pattern) => segmentsMatch(pattern, parts));
    };
    compiled.set(patterns, matcher);
  }
  return matcher;
}

export function pathGlobMatches(pattern: string, path: string): boolean {
  return pathGlobMatcher([pattern])(path);
}
