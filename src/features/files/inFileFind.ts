import { utf8ByteLength } from "../../domain/text";
import {
  MAX_EDITABLE_CONTENT_BYTES,
  MAX_FIND_QUERY_LENGTH,
} from "./editorContent";

export const MAX_FIND_MATCHES = 10_000;
export const REGEX_FIND_TIMEOUT_MS = 250;

export interface FindOptions {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
}

/** Messages never repeat the pattern. */
export type FindError =
  "invalid" | "inlineFlag" | "empty" | "timeout" | "unavailable";

export interface FindMatches {
  ranges: number[];
  truncated: boolean;
  contentLimitExceeded: boolean;
  error?: FindError;
  inserts?: string[];
}

export interface FindRequest {
  content: string;
  query: string;
  options: FindOptions;
  replacement?: string;
  limit?: number;
}

export const NO_MATCHES: FindMatches = {
  ranges: [],
  truncated: false,
  contentLimitExceeded: false,
};
const WORD = /[\p{L}\p{N}_]/u;

function escapeRegularExpression(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

function wordAt(content: string, index: number): boolean {
  return index >= 0 && index < content.length && WORD.test(content[index]!);
}

/**
 * IDE-0002's rules, on the WebView's engine: the request's case setting wins,
 * so inline flag groups such as `(?i:…)` are refused, and a pattern that can
 * match empty text is refused rather than yielding endless empty matches.
 */
function compile(query: string, options: FindOptions): RegExp | FindError {
  const flags = options.caseSensitive ? "gu" : "giu";
  if (!options.regex) return new RegExp(escapeRegularExpression(query), flags);
  if (/\(\?[a-z-]+:/u.test(query.replace(/\\./gu, ""))) return "inlineFlag";
  try {
    const expression = new RegExp(query, flags);
    return expression.test("") ? "empty" : expression;
  } catch {
    return "invalid";
  }
}

function expand(replacement: string, match: RegExpExecArray): string {
  return replacement.replace(
    /\$(\$|&|<([^>]*)>|\d{1,2})|\\([nt\\])/gu,
    (token, dollar?: string, name?: string, escape?: string) => {
      if (escape) return escape === "n" ? "\n" : escape === "t" ? "\t" : "\\";
      if (dollar === "$") return "$";
      if (dollar === "&") return match[0];
      if (name !== undefined) return match.groups?.[name] ?? "";
      const group = Number(dollar);
      return group > 0 && group < match.length ? (match[group] ?? "") : token;
    },
  );
}

export function findMatches(request: FindRequest): FindMatches {
  const { content, query, options, replacement } = request;
  const limit = request.limit ?? MAX_FIND_MATCHES;
  // Check UTF-16 length first so exact UTF-8 validation never copies an
  // unbounded string.
  if (content.length > MAX_EDITABLE_CONTENT_BYTES) {
    return { ...NO_MATCHES, contentLimitExceeded: true };
  }
  if (query.length === 0 || query.length > MAX_FIND_QUERY_LENGTH) {
    return NO_MATCHES;
  }
  if (utf8ByteLength(content) > MAX_EDITABLE_CONTENT_BYTES) {
    return { ...NO_MATCHES, contentLimitExceeded: true };
  }
  const expression = compile(query, options);
  if (typeof expression === "string") {
    return { ...NO_MATCHES, error: expression };
  }
  const ranges: number[] = [];
  const inserts: string[] = [];
  const result = () =>
    replacement === undefined ? { ranges } : { ranges, inserts };
  for (let lineStart = 0; lineStart <= content.length;) {
    const newline = options.regex ? content.indexOf("\n", lineStart) : -1;
    const lineEnd = newline < 0 ? content.length : newline;
    const line = options.regex
      ? content.slice(
          lineStart,
          content[lineEnd - 1] === "\r" ? lineEnd - 1 : lineEnd,
        )
      : content;
    for (const match of line.matchAll(expression)) {
      const from = lineStart + match.index;
      const to = from + match[0].length;
      if (from === to) return { ...NO_MATCHES, error: "empty" };
      if (
        options.wholeWord &&
        (wordAt(content, from - 1) || wordAt(content, to))
      ) {
        continue;
      }
      if (ranges.length === limit * 2) {
        return { ...NO_MATCHES, ...result(), truncated: true };
      }
      ranges.push(from, to);
      if (replacement !== undefined) {
        inserts.push(options.regex ? expand(replacement, match) : replacement);
      }
    }
    if (newline < 0) break;
    lineStart = newline + 1;
  }
  return { ...NO_MATCHES, ...result() };
}

type Settle = (result: FindMatches | null) => void;

/**
 * The deadline terminates a busy worker, so a runaway pattern costs the main
 * thread nothing.
 */
export function createFindWorkerClient(
  spawn: () => Worker,
  timeoutMs = REGEX_FIND_TIMEOUT_MS,
) {
  let worker: Worker | null = null;
  let running: { settle: Settle; timer: ReturnType<typeof setTimeout> } | null =
    null;
  let waiting: { request: FindRequest; settle: Settle } | null = null;
  const finish = (result: FindMatches | null) => {
    if (!running) return;
    clearTimeout(running.timer);
    const { settle } = running;
    running = null;
    settle(result);
    const next = waiting;
    waiting = null;
    if (next) start(next.request, next.settle);
  };
  const stop = (result: FindMatches | null) => {
    worker?.terminate();
    worker = null;
    finish(result);
  };
  function start(request: FindRequest, settle: Settle) {
    const timer = setTimeout(
      () => stop({ ...NO_MATCHES, error: "timeout" }),
      timeoutMs,
    );
    running = { settle, timer };
    try {
      if (!worker) {
        worker = spawn();
        worker.onmessage = (event: MessageEvent<FindMatches>) =>
          finish(event.data);
        worker.onerror = () => stop({ ...NO_MATCHES, error: "unavailable" });
      }
      worker.postMessage(request);
    } catch {
      stop({ ...NO_MATCHES, error: "unavailable" });
    }
  }
  return {
    run(request: FindRequest): Promise<FindMatches | null> {
      return new Promise((settle) => {
        if (!running) return start(request, settle);
        waiting?.settle(null);
        waiting = { request, settle };
      });
    },
    dispose() {
      waiting?.settle(null);
      waiting = null;
      stop(null);
    },
  };
}
