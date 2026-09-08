export const MAX_FIND_QUERY_LENGTH = 256;
export const MAX_FIND_MATCHES = 10_000;
export const MAX_EDITABLE_CONTENT_BYTES = 1024 * 1024;

import { utf8ByteLength } from "../../domain/text";

export interface FindMatches {
  offsets: number[];
  truncated: boolean;
  contentLimitExceeded: boolean;
}

function escapeRegularExpression(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

export function findLiteralMatches(
  content: string,
  query: string,
  caseSensitive: boolean,
): FindMatches {
  // Mirrors editor_content::MAX_EDITABLE_BYTES. Check UTF-16 length first so
  // exact UTF-8 validation never copies an unbounded string.
  if (content.length > MAX_EDITABLE_CONTENT_BYTES) {
    return { offsets: [], truncated: false, contentLimitExceeded: true };
  }

  if (query.length === 0 || query.length > MAX_FIND_QUERY_LENGTH) {
    return { offsets: [], truncated: false, contentLimitExceeded: false };
  }

  if (utf8ByteLength(content) > MAX_EDITABLE_CONTENT_BYTES) {
    return { offsets: [], truncated: false, contentLimitExceeded: true };
  }

  const expression = new RegExp(
    escapeRegularExpression(query),
    caseSensitive ? "gu" : "giu",
  );
  const offsets: number[] = [];
  for (const match of content.matchAll(expression)) {
    if (offsets.length === MAX_FIND_MATCHES) {
      return { offsets, truncated: true, contentLimitExceeded: false };
    }
    offsets.push(match.index);
  }
  return { offsets, truncated: false, contentLimitExceeded: false };
}
