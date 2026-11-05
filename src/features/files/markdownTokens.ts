import MarkdownIt, { type Token } from "markdown-it";
import type {
  MarkdownPreviewBlock,
  MarkdownPreviewDocument,
} from "./markdownPreview";
import type { ListItemShape, MarkdownListBlock } from "./markdownListTypes";
import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_LIST_DEPTH,
  MAX_MARKDOWN_TABLE_COLUMNS,
  MAX_MARKDOWN_TABLE_ROWS,
} from "./markdownLimits";

// This module belongs to the demand-loaded markdown renderer. The instance
// retains rules only: tokens, environments and source maps are per invocation.
const parser = new MarkdownIt({
  html: false,
  linkify: false,
  typographer: false,
  // Each list level owns a list and an item; admit their leaf paragraph.
  maxNesting: MAX_MARKDOWN_LIST_DEPTH * 2 + 2,
});

interface BlockBudget {
  limit: number;
  count: number;
  cursor: number;
  truncated: boolean;
}

// Run before any allocating rule. Container recursion shares the environment,
// but only completed top-level blocks consume the visible block allowance.
parser.block.ruler.before(
  "table",
  "sage_blocks",
  (state, _start, end, silent) => {
    if (silent || state.level !== 0) return false;
    const budget = state.env.sageBlockBudget as BlockBudget;
    for (; budget.cursor < state.tokens.length; budget.cursor++) {
      const token = state.tokens[budget.cursor];
      if (token.level === 0 && token.nesting >= 0) budget.count++;
    }
    if (budget.count < budget.limit) return false;
    budget.truncated = true;
    state.line = end;
    return true;
  },
);

// Preserve Sage's bounded inline presentation and file-link admission rules.
// markdown-it owns block classification, never HTML rendering.
parser.core.ruler.disable([
  "inline",
  "linkify",
  "replacements",
  "smartquotes",
  "text_join",
]);
parser.core.ruler.after("block", "sage_tasks", ({ tokens }) => {
  for (let i = 0; i < tokens.length - 2; i++) {
    if (
      tokens[i].type !== "list_item_open" ||
      tokens[i + 1].type !== "paragraph_open" ||
      tokens[i + 2].type !== "inline"
    )
      continue;
    const task = /^\[([ xX])\][ \t\n]+(?=\S)/u.exec(tokens[i + 2].content);
    if (!task) continue;
    tokens[i].meta = { checked: task[1] !== " " };
    tokens[i + 2].content = tokens[i + 2].content.slice(task[0].length);
  }
});

// At the visible list-depth cap, keep further container syntax as literal
// paragraph content instead of allocating deeper containers or losing the tail.
parser.block.ruler.before(
  "list",
  "sage_depth",
  (state, start, _end, silent) => {
    if (state.level < MAX_MARKDOWN_LIST_DEPTH * 2) return false;
    if (silent) return true;
    const content = state
      .getLines(start, start + 1, state.blkIndent, false)
      .trim();
    const open = state.push("paragraph_open", "p", 1);
    open.map = [start, start + 1];
    const token = state.push("inline", "", 0);
    token.content = content;
    token.map = [start, start + 1];
    state.push("paragraph_close", "p", -1);
    state.line = start + 1;
    return true;
  },
);

// A source position for each visible top-level block, without retaining tokens.
// Weak keys let unmounted documents and their offsets be collected together.
const positions = new WeakMap<
  MarkdownPreviewDocument,
  { offsets: number[]; truncated: boolean[] }
>();
export const markdownBlockOffsets = (document: MarkdownPreviewDocument) =>
  positions.get(document)?.offsets ?? [];

export function parseMarkdownTokens(
  source: string,
  blockLimit = MAX_MARKDOWN_PREVIEW_BLOCKS,
) {
  const sageBlockBudget: BlockBudget = {
    limit: blockLimit,
    count: 0,
    cursor: 0,
    truncated: false,
  };
  const tokens = parser.parse(source, { sageBlockBudget });
  return { tokens, truncated: sageBlockBudget.truncated };
}

export function retainMarkdownPrefix(
  document: MarkdownPreviewDocument,
  count: number,
): MarkdownPreviewDocument {
  const prefix = {
    blocks: document.blocks.slice(0, count),
    formats: document.formats.filter((format) => format.blockIndex < count),
    fileLinks: document.fileLinks.filter((link) => link.blockIndex < count),
    truncated:
      count > 0 && (positions.get(document)?.truncated[count - 1] ?? false),
    sourceLimitExceeded: false,
  };
  positions.set(prefix, {
    offsets: markdownBlockOffsets(document).slice(0, count),
    truncated: positions.get(document)?.truncated.slice(0, count) ?? [],
  });
  return prefix;
}

export function adaptMarkdownTokens(
  source: string,
  document: MarkdownPreviewDocument,
  inline: (source: string, itemIndex?: number) => string,
  reset: () => void,
  prefix?: MarkdownPreviewDocument,
  sourceOffset = 0,
) {
  const normalized = source.replace(/\r\n?/gu, "\n");
  const { tokens, truncated: tokenLimitExceeded } = parseMarkdownTokens(
    normalized,
    MAX_MARKDOWN_PREVIEW_BLOCKS - document.blocks.length,
  );
  const offsets = [0];
  for (let i = 0; i < normalized.length; i++)
    if (normalized[i] === "\n") offsets.push(i + 1);
  const starts = prefix ? [...markdownBlockOffsets(prefix)] : [];
  const truncated = prefix ? [...(positions.get(prefix)?.truncated ?? [])] : [];
  positions.set(document, { offsets: starts, truncated });
  const blocks = document.blocks as MarkdownPreviewBlock[];
  let cursor = 0;
  const closing = (from: number) => {
    const level = tokens[from].level;
    let end = from + 1;
    while (
      end < tokens.length &&
      !(tokens[end].level === level && tokens[end].nesting === -1)
    )
      end++;
    return end;
  };
  const push = (block: MarkdownPreviewBlock, token: Token) => {
    blocks.push(block);
    starts.push(sourceOffset + offsets[token.map?.[0] ?? 0]);
    truncated.push(document.truncated);
  };
  const code = (token: Token): MarkdownPreviewBlock => {
    // The token's end map includes a closer only if one was consumed. Compare
    // the body line count instead of reimplementing container/fence syntax.
    let bodyLines = Number(!!token.content && !token.content.endsWith("\n"));
    for (let i = 0; i < token.content.length; i++)
      if (token.content[i] === "\n") bodyLines++;
    const fenceClosed =
      token.type === "code_block" ||
      (!!token.map && token.map[1] - token.map[0] > bodyLines + 1);
    const text = fenceClosed
      ? token.content.replace(/\n$/u, "")
      : token.content;
    if (text.length > MAX_MARKDOWN_PREVIEW_BLOCK_CHARS)
      document.truncated = true;
    return {
      kind: "code",
      text: text.slice(0, MAX_MARKDOWN_PREVIEW_BLOCK_CHARS),
      language: token.info.trim().split(/\s/u)[0],
      fenceClosed,
    };
  };
  while (cursor < tokens.length) {
    if (blocks.length === MAX_MARKDOWN_PREVIEW_BLOCKS) {
      document.truncated = true;
      break;
    }
    reset();
    const token = tokens[cursor];
    const end = token.nesting === 1 ? closing(cursor) : cursor;
    if (token.type === "fence" || token.type === "code_block")
      push(code(token), token);
    else if (token.type === "hr") push({ kind: "rule" }, token);
    else if (token.type === "heading_open" || token.type === "paragraph_open") {
      const text = inline(tokens[cursor + 1].content);
      if (token.type === "heading_open")
        push(
          { kind: "heading", level: Number(token.tag.slice(1)) as 1, text },
          token,
        );
      else if (text) push({ kind: "paragraph", text }, token);
    } else if (token.type === "blockquote_open") {
      // The existing quote component intentionally presents a literal inline
      // quote. Complex child blocks are excluded from structural conformance.
      const raw = normalized
        .slice(
          offsets[token.map![0]],
          offsets[token.map![1]] ?? normalized.length,
        )
        .split("\n")
        .map((line) => line.replace(/^ {0,3}>[ \t]?/u, ""))
        .join(" ");
      push({ kind: "quote", text: inline(raw) }, token);
    } else if (
      token.type === "bullet_list_open" ||
      token.type === "ordered_list_open"
    ) {
      const items: string[] = [];
      const shape: ListItemShape[] = [];
      const levels: { token: Token; first: boolean }[] = [];
      const children: Token[] = [];
      for (let i = cursor; i <= end; i++) {
        const current = tokens[i];
        if (
          current.type === "bullet_list_open" ||
          current.type === "ordered_list_open"
        )
          levels.push({ token: current, first: true });
        else if (
          current.type === "bullet_list_close" ||
          current.type === "ordered_list_close"
        )
          levels.pop();
        else if (current.type === "list_item_open") {
          const itemEnd = closing(i);
          const raw: string[] = [];
          for (let j = i + 1; j < itemEnd; j++) {
            if (
              tokens[j].level <= current.level + 2 &&
              tokens[j].type === "inline"
            )
              raw.push(tokens[j].content);
          }
          if (items.length >= MAX_MARKDOWN_PREVIEW_BLOCK_CHARS) {
            document.truncated = true;
            break;
          }
          const text = inline(raw.join(" "), items.length);
          if (!text && raw.join("").trim()) continue;
          const level = levels.at(-1)!;
          const depth = Math.min(
            levels.length - 1,
            MAX_MARKDOWN_LIST_DEPTH - 1,
            items.length ? shape.at(-1)!.depth + 1 : 0,
          );
          const ordered = level.token.type === "ordered_list_open";
          const entry: ListItemShape = { depth, ordered };
          if (typeof current.meta?.checked === "boolean")
            entry.checked = current.meta.checked;
          if (depth && ordered && level.first && level.token.attrGet("start"))
            entry.start = Number(level.token.attrGet("start"));
          if (depth && level.first && shape.some((s) => s.depth === depth))
            entry.newList = true;
          items.push(text);
          shape.push(entry);
          level.first = false;
          if (levels.length > MAX_MARKDOWN_LIST_DEPTH)
            document.truncated = true;
        } else if (
          current.type === "fence" ||
          current.type === "code_block" ||
          current.type === "hr"
        )
          if (children.length < MAX_MARKDOWN_PREVIEW_BLOCKS)
            children.push(current);
          else document.truncated = true;
      }
      const list: MarkdownListBlock = { kind: "list", items };
      if (token.type === "ordered_list_open") {
        list.ordered = true;
        list.start = Number(token.attrGet("start") ?? 1);
      }
      if (shape.some((s) => s.depth || s.checked !== undefined))
        list.shape = shape;
      push(list, token);
      for (const child of children) {
        if (blocks.length === MAX_MARKDOWN_PREVIEW_BLOCKS) {
          document.truncated = true;
          break;
        }
        push(child.type === "hr" ? { kind: "rule" } : code(child), token);
      }
    } else if (token.type === "table_open") {
      const align: ("left" | "center" | "right" | undefined)[] = [];
      const rows: string[][] = [];
      let column = 0;
      let rowIndex = -1;
      for (let i = cursor + 1; i < end; i++) {
        const current = tokens[i];
        if (current.type === "tr_open") {
          column = 0;
          rowIndex++;
          if (rowIndex <= MAX_MARKDOWN_TABLE_ROWS) rows.push([]);
          else document.truncated = true;
        }
        if (current.type !== "th_open" && current.type !== "td_open") continue;
        if (
          column >= MAX_MARKDOWN_TABLE_COLUMNS ||
          rowIndex > MAX_MARKDOWN_TABLE_ROWS
        ) {
          document.truncated = true;
          column++;
          continue;
        }
        if (current.type === "th_open")
          align.push(
            current.attrGet("style")
              ? (String(current.attrGet("style")).replace(
                  "text-align:",
                  "",
                ) as (typeof align)[number])
              : undefined,
          );
        rows
          .at(-1)!
          .push(
            inline(
              tokens[i + 1].content,
              (rows.length - 1) * align.length + column,
            ),
          );
        column++;
      }
      push({ kind: "table", align, rows }, token);
    }
    cursor = end + 1;
  }
  // Keep parser overflow out of per-block prefix snapshots: a later streaming
  // suffix may replace the truncated tail while retaining the earlier blocks.
  document.truncated ||= tokenLimitExceeded;
}
