import type { Token } from "markdown-it";
import type { MarkdownBlock } from "./markdownBlockTypes";
import {
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_TABLE_ROWS,
} from "./markdownLimits";

export type MarkdownRawBlock = MarkdownBlock;
type Block = MarkdownRawBlock;

interface Node {
  token: Token;
  children: Node[];
}

// Container identity comes from matching open/close types, never from an
// assumed distance between token levels.
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

// Containers always use adapt() so their block identity and code whitespace
// never pass through text joining.
function inlineText(node: Node): string {
  return node.token.type === "inline"
    ? node.token.content
    : node.children.map(inlineText).join(" ");
}

function adapt(node: Node, overflow: () => void): Block[] {
  const { token, children } = node;
  const descendants = () => children.flatMap((child) => adapt(child, overflow));
  switch (token.type) {
    case "inline":
      return [{ kind: "paragraph", text: token.content }];
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
          text: inlineText(node),
        },
      ];
    case "blockquote_open":
      return [{ kind: "quote", children: descendants() }];
    case "bullet_list_open":
    case "ordered_list_open": {
      const ordered = token.type === "ordered_list_open";
      return [
        {
          kind: "list",
          items: children.map((item) => ({
            children: adapt(item, overflow),
            ...(typeof item.token.meta?.checked === "boolean"
              ? { checked: item.token.meta.checked }
              : {}),
          })),
          ...(children.some((item) =>
            item.children.some(
              (child) =>
                child.token.type === "paragraph_open" && !child.token.hidden,
            ),
          )
            ? { loose: true }
            : {}),
          ...(ordered
            ? { ordered: true, start: Number(token.attrGet("start") ?? 1) }
            : {}),
        },
      ];
    }
    case "table_open": {
      if (token.meta?.truncated) overflow();
      const align: Extract<Block, { kind: "table" }>["align"] = [];
      const rows: string[][] = [];
      const visit = (current: Node) => {
        if (current.token.type !== "tr_open") {
          current.children.forEach(visit);
          return;
        }
        if (rows.length > MAX_MARKDOWN_TABLE_ROWS) {
          overflow();
          return;
        }
        const row: string[] = [];
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
          row.push(inlineText(cell));
        }
        rows.push(row);
      };
      visit(node);
      return [{ kind: "table", align, rows }];
    }
    default:
      return children.length
        ? descendants()
        : token.content
          ? [{ kind: "paragraph", text: token.content }]
          : [];
  }
}
