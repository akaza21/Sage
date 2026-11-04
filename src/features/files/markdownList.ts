import { MAX_MARKDOWN_LIST_DEPTH } from "./markdownLimits";

export interface ListItemShape {
  depth: number;
  ordered: boolean;
  checked?: boolean;
}

export interface MarkdownListBlock {
  kind: "list";
  items: readonly string[];
  ordered?: true;
  start?: number;
  /** Present only when an item is nested or is a task. */
  shape?: readonly ListItemShape[];
}

interface ListItem {
  indent: number;
  content: number;
  hasSeparator: boolean;
  ordered: boolean;
  start: number;
  checked?: boolean;
  text: string;
}

/** A thematic break takes precedence over a list marker. */
export function isMarkdownRule(line: string): boolean {
  return /^ {0,3}(?:(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,})$/u.test(
    line,
  );
}

/** Markdown tabs advance to the next four-column stop. */
function columnAfter(text: string, column = 0): number {
  for (const character of text)
    column += character === "\t" ? 4 - (column % 4) : 1;
  return column;
}

export function listItem(line: string): ListItem | null {
  if (isMarkdownRule(line)) return null;
  const match = /^([ \t]*)(?:([-*+])|(\d{1,9})[.)])(?:([ \t]+)(.*))?$/u.exec(
    line,
  );
  if (!match) return null;
  const indent = columnAfter(match[1]);
  const markerEnd = indent + (match[2] ? 1 : match[3].length + 1);
  const separatorWidth = columnAfter(match[4] ?? "", markerEnd) - markerEnd;
  const text = match[5] ?? "";
  const task = /^\[([ xX])\][ \t]+(\S.*)$/u.exec(text);
  return {
    indent,
    // Blank items and over-wide separators use one column of list padding.
    content: markerEnd + (text && separatorWidth <= 4 ? separatorWidth : 1),
    hasSeparator: match[4] !== undefined,
    ordered: !match[2],
    start: Number(match[3] ?? 1),
    ...(task ? { checked: task[1] !== " " } : {}),
    text: task ? task[2] : text,
  };
}

/** Keep paragraph interruption limited to unindented, separator-bearing markers. */
export function interruptsParagraph(line: string): boolean {
  const item = listItem(line);
  return !!item && item.indent < 4 && item.hasSeparator;
}

/**
 * Reads the list that starts at `lines[cursor]`. An item indented to the
 * content column of the item above it at its level nests one level deeper,
 * up to the depth limit. `read` returns null when its budget is exhausted.
 * Empty source items are retained; other items whose rendered text is empty
 * are dropped. A kept item is never more than one level below the kept item
 * before it, so the flat list always rebuilds into a well-formed tree.
 */
export function readMarkdownList(
  lines: readonly string[],
  cursor: number,
  read: (text: string, itemIndex: number) => string | null,
): { block: MarkdownListBlock; cursor: number } | null {
  const first = listItem(lines[cursor].trimEnd());
  if (!first || first.indent >= 4) return null;
  const items: string[] = [];
  const shape: ListItemShape[] = [];
  // Each level's indent and the content column of its latest item.
  const levels = [{ indent: first.indent, content: first.content }];
  let next: ListItem | null = first;
  do {
    while (levels.length > 1 && next.indent < levels.at(-1)!.indent)
      levels.pop();
    if (
      next !== first &&
      next.indent >= levels.at(-1)!.content &&
      levels.length < MAX_MARKDOWN_LIST_DEPTH
    )
      levels.push({ indent: next.indent, content: next.content });
    else levels.at(-1)!.content = next.content;
    const text = read(next.text, items.length);
    if (text || (text === "" && !next.text.trim())) {
      const depth = Math.min(
        levels.length - 1,
        items.length ? shape.at(-1)!.depth + 1 : 0,
      );
      items.push(text);
      shape.push({ depth, ordered: next.ordered });
      if (next.checked !== undefined) shape.at(-1)!.checked = next.checked;
    }
    next = listItem(lines[++cursor] ?? "");
  } while (
    next &&
    (next.indent >= first.content || next.ordered === first.ordered)
  );
  return {
    block: {
      kind: "list",
      items,
      ...(first.ordered ? { ordered: true, start: first.start } : {}),
      ...(shape.some((entry) => entry.depth || entry.checked !== undefined)
        ? { shape }
        : {}),
    },
    cursor,
  };
}
