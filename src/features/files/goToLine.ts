import { fileNavigationSelection } from "./fileNavigation";
import { MAX_GOTO_LINE_INPUT_CHARS } from "./goToLineLimits";

export { MAX_GOTO_LINE_INPUT_CHARS } from "./goToLineLimits";
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
