import type { Text } from "@codemirror/state";
import { fileNavigationSelection } from "./fileNavigation";

export const MAX_GOTO_LINE_INPUT_CHARS = 32;
export const MAX_GOTO_LINE_POSITION = 0xffff_ffff;

export function parseGoToLineInput(raw: string): {
  lineNumber: number;
  columnNumber: number;
} | null {
  if (raw.length === 0 || raw.length > MAX_GOTO_LINE_INPUT_CHARS) {
    return null;
  }
  const trimmed = raw.trim();
  if (trimmed.length === 0 || trimmed.length > MAX_GOTO_LINE_INPUT_CHARS) {
    return null;
  }
  const match = /^(\d+)(?::(\d+))?$/u.exec(trimmed);
  if (!match) {
    return null;
  }
  const lineNumber = Number(match[1]);
  const columnNumber = match[2] === undefined ? 1 : Number(match[2]);
  if (
    !Number.isSafeInteger(lineNumber) ||
    lineNumber < 1 ||
    lineNumber > MAX_GOTO_LINE_POSITION
  ) {
    return null;
  }
  if (
    !Number.isSafeInteger(columnNumber) ||
    columnNumber < 1 ||
    columnNumber > MAX_GOTO_LINE_POSITION
  ) {
    return null;
  }
  return { lineNumber, columnNumber };
}

export function goToLineSelection(content: string, raw: string) {
  const parsed = parseGoToLineInput(raw);
  if (!parsed) {
    return null;
  }
  return fileNavigationSelection(content, {
    ...parsed,
    selection: { kind: "caret" },
  });
}

export interface EditorCaret {
  line: number;
  column: number;
}

export function admitCaretStatus(
  document: Text,
  offset: number,
): EditorCaret | null {
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > document.length) {
    return null;
  }
  // The editor's document finds the line in its tree, without a scan.
  const line = document.lineAt(offset);
  const column = offset - line.from + 1;
  if (line.number > MAX_GOTO_LINE_POSITION || column > MAX_GOTO_LINE_POSITION) {
    return null;
  }
  return { line: line.number, column };
}

export function caretStatusLabel(caret: EditorCaret | null): string | null {
  if (
    caret === null ||
    !Number.isSafeInteger(caret.line) ||
    !Number.isSafeInteger(caret.column) ||
    caret.line < 1 ||
    caret.column < 1 ||
    caret.line > MAX_GOTO_LINE_POSITION ||
    caret.column > MAX_GOTO_LINE_POSITION
  ) {
    return null;
  }
  return `Ln ${caret.line}, Col ${caret.column}`;
}
