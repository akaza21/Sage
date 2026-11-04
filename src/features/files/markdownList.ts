import { MAX_MARKDOWN_LIST_DEPTH } from "./markdownLimits";
import {
  columnAfter,
  indentation,
  isMarkdownRule,
  startsLeafBlock,
  stripIndent,
} from "./markdownSyntax";

export interface ListItemShape {
  depth: number;
  ordered: boolean;
  checked?: boolean;
  /** Non-default start of a nested ordered list. */
  start?: number;
  /** A different bullet or ordered delimiter starts another list. */
  newList?: true;
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
  marker: string;
  ordered: boolean;
  start: number;
  text: string;
}

export function listItem(line: string, initialColumn = 0): ListItem | null {
  if (isMarkdownRule(line)) return null;
  const match = /^([ \t]*)(?:([-*+])|(\d{1,9})([.)]))(?:([ \t]+)(.*))?$/u.exec(
    line,
  );
  if (!match) return null;
  const indent = columnAfter(match[1], initialColumn);
  const markerEnd = indent + (match[2] ? 1 : match[3].length + 1);
  const separatorWidth = columnAfter(match[5] ?? "", markerEnd) - markerEnd;
  const text = match[6] ?? "";
  return {
    indent,
    // Blank items and over-wide separators use one column of list padding.
    content: markerEnd + (text && separatorWidth <= 4 ? separatorWidth : 1),
    marker: match[2] ?? match[4],
    ordered: !match[2],
    start: Number(match[3] ?? 1),
    text,
  };
}

/** Empty items and ordered starts other than 1 cannot interrupt paragraphs. */
export function interruptsParagraph(line: string): boolean {
  const item = listItem(line);
  return (
    !!item &&
    item.indent < 4 &&
    !!item.text.trim() &&
    (!item.ordered || item.start === 1)
  );
}

/**
 * The stack stores each level's latest item, so both admission and nesting
 * use the current content column even when marker widths or padding change.
 * Only paragraph text and nested lists belong to this bounded flat subset.
 * Other child blocks are left for the preview's leaf-block reader.
 */
export function readMarkdownList(
  lines: readonly string[],
  cursor: number,
  read: (text: string, itemIndex: number) => string | null,
): { block: MarkdownListBlock; cursor: number; contentIndent: number } | null {
  const first = listItem(lines[cursor]);
  if (!first || first.indent >= 4) return null;
  const items: string[] = [];
  const shape: ListItemShape[] = [];
  const levels: ListItem[] = [];
  let next: ListItem | null = first;
  let contentIndent = 0;
  while (next) {
    while (levels.length > 1 && next.indent < levels.at(-2)!.content)
      levels.pop();
    const parent = levels.at(-1);
    const nested = parent && next.indent >= parent.content;
    if (nested && levels.length < MAX_MARKDOWN_LIST_DEPTH) levels.push(next);
    else {
      if (levels.length === 1 && next.marker !== first.marker) break;
      levels[levels.length ? levels.length - 1 : 0] = next;
    }
    const newList = !nested && parent && parent.marker !== next.marker;
    const base = levels.at(-2)?.content ?? 0;
    contentIndent = next.content;
    // Bound same-line descent as well as the retained tree. Tab stops stay
    // anchored to the original line, including preceding container markers.
    const inner: ListItem | null =
      levels.length < MAX_MARKDOWN_LIST_DEPTH
        ? listItem(next.text, next.content)
        : null;
    // Collected text opens a paragraph, even after an empty opening marker.
    const parts = inner || !next.text ? [] : [next.text];
    const startsWithLeaf = startsLeafBlock(next.text);
    if (!inner) {
      cursor++;
      // A lazy paragraph continuation need not carry container indentation.
      // Indented markers still need to be within three columns of their
      // containing level; otherwise they are paragraph text (spec 312).
      while (!startsWithLeaf && cursor < lines.length && lines[cursor].trim()) {
        const line = lines[cursor];
        const candidate = listItem(line);
        if (
          candidate &&
          (!parts.length ||
            (candidate.indent < next.content && candidate.indent < base + 4))
        )
          break;
        const relative =
          indentation(line) >= next.content
            ? stripIndent(line, next.content)
            : line;
        // A new nested list must interrupt this item's paragraph; siblings
        // continue an existing list even when their marker has no content.
        if (startsLeafBlock(relative) || interruptsParagraph(relative)) break;
        if (!parts.length && indentation(line) < next.content) break;
        parts.push(relative);
        cursor++;
      }
    }
    const raw = parts.join(" ");
    // GFM tasks decorate a paragraph after its container has been classified.
    // Removing the checkbox earlier would reinterpret a label like "- literal"
    // as a same-line nested list.
    const task = /^\[([ xX])\][ \t]+(\S.*)$/u.exec(raw);
    const text = read(task ? task[2] : raw, items.length);
    if (text || (text === "" && !raw.trim())) {
      const depth = Math.min(
        levels.length - 1,
        items.length ? shape.at(-1)!.depth + 1 : 0,
      );
      items.push(text);
      const entry: ListItemShape = { depth, ordered: next.ordered };
      if (task) entry.checked = task[1] !== " ";
      if (depth && next.ordered && next.start !== 1) entry.start = next.start;
      if (newList) entry.newList = true;
      shape.push(entry);
    }
    if (inner) {
      next = inner;
      continue;
    }
    while (cursor < lines.length && !lines[cursor].trim()) cursor++;
    next = listItem(lines[cursor] ?? "");
    if (!next) break;
    if (next.indent >= base + 4 && next.indent < contentIndent) break;
  }
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
    contentIndent,
  };
}
