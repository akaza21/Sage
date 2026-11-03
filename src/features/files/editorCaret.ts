import type { Text } from "@codemirror/state";
import { MAX_GOTO_LINE_POSITION } from "./goToLine";

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
