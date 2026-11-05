import type { Token } from "markdown-it";
import type { MarkdownPreviewBlock } from "./markdownPreview";
import type { ListItemShape } from "./markdownListTypes";
import {
  MAX_MARKDOWN_LIST_DEPTH,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_TABLE_ROWS,
} from "./markdownLimits";

export type MarkdownContent = (string | { literal: string })[];
type Block =
  | { kind: "paragraph" | "quote"; text: MarkdownContent }
  | { kind: "heading"; level: 1; text: MarkdownContent }
  | { kind: "rule" }
  | (Extract<MarkdownPreviewBlock, { kind: "code" }> & { markup: string })
  | {
      kind: "list";
      items: MarkdownContent[];
      shape: ListItemShape[];
      ordered?: true;
      start?: number;
    }
  | {
      kind: "table";
      align: ("left" | "center" | "right" | undefined)[];
      rows: MarkdownContent[][];
    };

interface Node {
  token: Token;
  children: Node[];
}

// Consume each token once. Container identity comes from matching open/close
// types, never from an assumed distance between token levels.
export function* markdownContainers(tokens: Token[], overflow: () => void) {
  let cursor = 0;
  const read = (): Node => {
    const token = tokens[cursor++];
    const children: Node[] = [];
    if (token.nesting === 1) {
      const close = token.type.replace(/_open$/u, "_close");
      while (cursor < tokens.length && tokens[cursor].type !== close)
        children.push(read());
      cursor++;
    }
    return { token, children };
  };
  while (cursor < tokens.length) {
    const node = read();
    yield { token: node.token, blocks: adapt(node, overflow) };
  }
}

function joined(parts: MarkdownContent[]): MarkdownContent {
  return parts.flatMap((part, index) => (index ? [" ", ...part] : part));
}

function content(block: Block): MarkdownContent {
  if (block.kind === "list") {
    const counters: (number | undefined)[] = [];
    return joined(
      block.items.map((item, index) => {
        const { depth, ordered, checked, start, newList } = block.shape[index];
        counters.length = Math.min(counters.length, depth + 1);
        let marker = "";
        if (ordered) {
          counters[depth] =
            start ??
            (newList || counters[depth] === undefined
              ? depth
                ? 1
                : (block.start ?? 1)
              : counters[depth]! + 1);
          marker = `${counters[depth]}. `;
        } else counters[depth] = undefined;
        if (checked !== undefined) marker += `[${checked ? "x" : " "}] `;
        return marker ? [marker, ...item] : item;
      }),
    );
  }
  if (block.kind === "table") return joined(block.rows.flat());
  if (block.kind === "rule") return ["---"];
  if (block.kind === "code") {
    // Quotes have no child-block slot. Preserve code literally, including an
    // unfinished fence, without admitting its markup as inline formatting.
    const raw = block.markup
      ? `${block.markup}${block.language}\n${block.text}${block.fenceClosed ? `\n${block.markup}` : ""}`
      : block.text;
    return [{ literal: raw.replace(/\s+/gu, " ").trim() }];
  }
  return block.text;
}

function adapt(node: Node, overflow: () => void): Block[] {
  const { token, children } = node;
  const descendants = () => children.flatMap((child) => adapt(child, overflow));
  switch (token.type) {
    case "inline":
      return [{ kind: "paragraph", text: [token.content] }];
    case "fence":
    case "code_block": {
      let lines = Number(!!token.content && !token.content.endsWith("\n"));
      for (const char of token.content) if (char === "\n") lines++;
      const fenceClosed =
        token.type === "code_block" ||
        (!!token.map && token.map[1] - token.map[0] > lines + 1);
      const text = fenceClosed
        ? token.content.replace(/\n$/u, "")
        : token.content;
      if (text.length > MAX_MARKDOWN_PREVIEW_BLOCK_CHARS) overflow();
      return [
        {
          kind: "code",
          text: text.slice(0, MAX_MARKDOWN_PREVIEW_BLOCK_CHARS),
          language: token.info.trim().split(/\s/u)[0],
          fenceClosed,
          markup: token.markup,
        },
      ];
    }
    case "hr":
      return [{ kind: "rule" }];
    case "heading_open":
      return [
        {
          kind: "heading",
          level: Number(token.tag.slice(1)) as 1,
          text: joined(descendants().map(content)),
        },
      ];
    case "blockquote_open":
      return [{ kind: "quote", text: joined(descendants().map(content)) }];
    case "bullet_list_open":
    case "ordered_list_open": {
      const items: MarkdownContent[] = [];
      const shape: ListItemShape[] = [];
      const following: Block[] = [];
      const ordered = token.type === "ordered_list_open";
      const seenDepths = new Set([0]);
      for (const item of children) {
        const blocks = adapt(item, overflow);
        items.push(
          joined(
            blocks
              .filter(
                (block) =>
                  block.kind === "paragraph" ||
                  block.kind === "heading" ||
                  block.kind === "quote",
              )
              .map(content),
          ),
        );
        const entry: ListItemShape = { depth: 0, ordered };
        if (typeof item.token.meta?.checked === "boolean")
          entry.checked = item.token.meta.checked;
        shape.push(entry);
        for (const block of blocks) {
          if (block.kind === "list") {
            for (let i = 0; i < block.items.length; i++) {
              const nested = block.shape[i];
              const depth = Math.min(
                nested.depth + 1,
                MAX_MARKDOWN_LIST_DEPTH - 1,
              );
              const start =
                i === 0 && block.ordered && block.start !== 1
                  ? { start: block.start }
                  : {};
              shape.push({
                ...nested,
                ...start,
                depth,
                ...(i === 0 && seenDepths.has(depth) ? { newList: true } : {}),
              });
              seenDepths.add(depth);
              items.push(block.items[i]);
            }
          } else if (
            block.kind === "code" ||
            block.kind === "rule" ||
            block.kind === "table"
          ) {
            if (following.length < MAX_MARKDOWN_PREVIEW_BLOCKS)
              following.push(block);
            else overflow();
          }
        }
      }
      return [
        {
          kind: "list",
          items,
          shape,
          ...(ordered
            ? { ordered: true, start: Number(token.attrGet("start") ?? 1) }
            : {}),
        },
        ...following,
      ];
    }
    case "table_open": {
      const align: Extract<Block, { kind: "table" }>["align"] = [];
      const rows: MarkdownContent[][] = [];
      const visit = (current: Node) => {
        if (current.token.type !== "tr_open") {
          current.children.forEach(visit);
          return;
        }
        if (rows.length > MAX_MARKDOWN_TABLE_ROWS) {
          overflow();
          return;
        }
        const row: MarkdownContent[] = [];
        for (const cell of current.children) {
          if (row.length === MAX_MARKDOWN_TABLE_COLUMNS) {
            overflow();
            break;
          }
          if (cell.token.type === "th_open") {
            const style = cell.token.attrGet("style");
            align.push(
              style
                ? (String(style).replace(
                    "text-align:",
                    "",
                  ) as (typeof align)[number])
                : undefined,
            );
          }
          row.push(joined(adapt(cell, overflow).map(content)));
        }
        rows.push(row);
      };
      visit(node);
      return [{ kind: "table", align, rows }];
    }
    default:
      // Paragraphs, list items, table sections/cells and future containers all
      // retain their children through the same adaptation path.
      return children.length
        ? descendants()
        : token.content
          ? [{ kind: "paragraph", text: [token.content] }]
          : [];
  }
}
