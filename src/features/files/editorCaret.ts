import { MAX_GOTO_LINE_POSITION } from "./goToLine";

export interface EditorCaret {
  line: number;
  column: number;
}

export function admitCaretStatus(
  content: string,
  offset: number,
): EditorCaret | null {
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > content.length) {
    return null;
  }
  let line = 1;
  let lineStart = 0;
  for (let index = 0; index < offset; index += 1) {
    if (content.charCodeAt(index) === 10) {
      line += 1;
      lineStart = index + 1;
      if (line > MAX_GOTO_LINE_POSITION) {
        return null;
      }
    }
  }
  const column = offset - lineStart + 1;
  if (column < 1 || column > MAX_GOTO_LINE_POSITION) {
    return null;
  }
  return { line, column };
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
