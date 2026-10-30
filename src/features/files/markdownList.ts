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
  ordered: boolean;
  start: number;
  checked?: boolean;
  text: string;
}

export function listItem(line: string): ListItem | null {
  const match = /^([ \t]*)(?:([-*+])|(\d{1,9})[.)]) (.+)$/u.exec(line);
  if (!match) return null;
  const indent = match[1].replace(/\t/gu, "    ").length;
  const task = /^\[([ xX])\][ \t]+(\S.*)$/u.exec(match[4]);
  return {
    indent,
    content: indent + (match[2] ?? match[3]).length + (match[2] ? 1 : 2),
    ordered: !match[2],
    start: Number(match[3] ?? 1),
    ...(task ? { checked: task[1] !== " " } : {}),
    text: task ? task[2] : match[4],
  };
}

/**
 * Reads the list that starts at `lines[cursor]`. An item indented to the
 * content column of the item above it at its level nests one level deeper,
 * up to the depth limit. `read` formats an item's text; an empty result drops
 * the item, and a kept item is never more than one level below the kept item
 * before it, so the flat list always rebuilds into a well-formed tree.
 */
export function readMarkdownList(
  lines: readonly string[],
  cursor: number,
  read: (text: string, itemIndex: number) => string,
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
    if (text) {
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
