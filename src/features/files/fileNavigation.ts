import { MAX_EDITABLE_CONTENT_BYTES } from "./editorContent";

export type FileNavigationSelectionIntent =
  { kind: "caret" } | { kind: "match"; query: string; caseSensitive: boolean };

interface FileNavigationLocation {
  lineNumber: number;
  columnNumber: number;
  selection: FileNavigationSelectionIntent;
}

function nextCodePointOffset(content: string, offset: number, limit: number) {
  const value = content.codePointAt(offset);
  return Math.min(
    limit,
    offset + (value !== undefined && value > 0xffff ? 2 : 1),
  );
}

function advanceCodePoints(
  content: string,
  offset: number,
  count: number,
  limit: number,
) {
  let next = offset;
  for (let index = 0; index < count && next < limit; index += 1) {
    next = nextCodePointOffset(content, next, limit);
  }
  return next;
}

function codePointCount(value: string) {
  let count = 0;
  for (const _character of value) count += 1;
  return count;
}

function matchedEnd(
  content: string,
  start: number,
  lineEnd: number,
  match: Extract<FileNavigationSelectionIntent, { kind: "match" }>,
) {
  const { query, caseSensitive } = match;
  if (caseSensitive && content.startsWith(query, start)) {
    return Math.min(lineEnd, start + query.length);
  }
  if (!caseSensitive) {
    const foldedQuery = query.toLowerCase();
    let folded = "";
    let end = start;
    while (end < lineEnd && folded.length < foldedQuery.length) {
      const next = nextCodePointOffset(content, end, lineEnd);
      folded += content.slice(end, next).toLowerCase();
      end = next;
    }
    if (folded.startsWith(foldedQuery)) return end;
  }
  return advanceCodePoints(content, start, codePointCount(query), lineEnd);
}

export function fileNavigationSelection(
  content: string,
  location: FileNavigationLocation,
) {
  const contentLimit = Math.min(content.length, MAX_EDITABLE_CONTENT_BYTES);
  let lineStart = 0;
  let currentLine = 1;
  for (
    let offset = 0;
    offset < contentLimit && currentLine < location.lineNumber;
    offset += 1
  ) {
    if (content.charCodeAt(offset) === 0x0a) {
      lineStart = offset + 1;
      currentLine += 1;
    }
  }
  let lineEnd = lineStart;
  while (lineEnd < contentLimit && content.charCodeAt(lineEnd) !== 0x0a) {
    lineEnd += 1;
  }
  if (lineEnd > lineStart && content.charCodeAt(lineEnd - 1) === 0x0d) {
    lineEnd -= 1;
  }
  const start = advanceCodePoints(
    content,
    lineStart,
    location.columnNumber - 1,
    lineEnd,
  );
  if (location.selection.kind === "caret") {
    return { start, end: start };
  }
  return {
    start,
    end: matchedEnd(content, start, lineEnd, location.selection),
  };
}
