import MarkdownIt from "markdown-it";
import {
  markdownContainers,
  type MarkdownRawBlock,
} from "./markdownContainers";
import type {
  MarkdownPreviewBlock,
  MarkdownPreviewDocument,
} from "./markdownPreview";
import {
  MAX_MARKDOWN_PREVIEW_BLOCKS,
  MAX_MARKDOWN_PREVIEW_BLOCK_CHARS,
  MAX_MARKDOWN_LIST_DEPTH,
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

// Five tokens per list item cover the character-sized item allowance, with
// headroom for containers. Unwinding may add maxNesting closing tokens.
const MAX_BLOCK_TOKENS = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS * 6;

interface BlockBudget {
  limit: number;
  count: number;
  cursor: number;
  truncated: boolean;
}

// Container recursion shares tokens and the environment. Check every level so
// a single list cannot exhaust memory before the visible block allowance.
parser.block.ruler.before(
  "table",
  "sage_blocks",
  (state, _start, _end, silent) => {
    if (silent) return false;
    const budget = state.env.sageBlockBudget as BlockBudget;
    for (; budget.cursor < state.tokens.length; budget.cursor++) {
      const token = state.tokens[budget.cursor];
      if (token.level === 0 && token.nesting >= 0) budget.count++;
    }
    if (
      !budget.truncated &&
      state.tokens.length < MAX_BLOCK_TOKENS &&
      (state.level !== 0 || budget.count < budget.limit)
    )
      return false;
    budget.truncated = true;
    // Let markdown-it emit container closers while unwinding. The sticky env
    // flag also stops enclosing tokenizers after a quote restores lineMax.
    state.line = state.lineMax;
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

// At the visible depth cap, keep further container syntax as literal paragraph
// content. Run before blockquotes as well as lists so maxNesting cannot discard
// their leaf text before this fallback gets a chance to preserve it.
parser.block.ruler.before(
  "blockquote",
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

// List-item prose retains the existing character/item allowance. Every other
// block, including nested containers and full code/table renderers, shares the
// document block cap. Recount only the bounded retained streaming prefix.
function blockCost(block: MarkdownPreviewBlock, listItem = false): number {
  let cost = Number(!(listItem && block.kind === "paragraph"));
  if (block.kind === "quote")
    for (const child of block.children) cost += blockCost(child);
  if (block.kind === "list")
    for (const item of block.items)
      for (const child of item.children) cost += blockCost(child, true);
  return cost;
}

export function adaptMarkdownTokens(
  source: string,
  document: MarkdownPreviewDocument,
  inline: (source: string, itemIndex?: number) => string,
  reset: () => void,
  literal: (text: string) => string,
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
  let remainingComponents =
    MAX_MARKDOWN_PREVIEW_BLOCKS -
    blocks.reduce((total, block) => total + blockCost(block), 0);
  let componentLimitExceeded = false;
  const overflow = () => {
    document.truncated = true;
  };
  containers: for (const container of markdownContainers(tokens, overflow)) {
    for (const raw of container.blocks) {
      if (blocks.length === MAX_MARKDOWN_PREVIEW_BLOCKS) {
        overflow();
        break containers;
      }
      reset();
      // One bounded budget per top-level block, shared by every descendant.
      let remainingBlocks = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS + 1;
      let remainingItems = MAX_MARKDOWN_PREVIEW_BLOCK_CHARS;
      let slot = 0;
      const adapt = (
        raw: MarkdownRawBlock,
        nested: boolean,
        listItem = false,
      ): MarkdownPreviewBlock | null => {
        const cost = Number(!(listItem && raw.kind === "paragraph"));
        if (componentLimitExceeded || (cost && !remainingComponents)) {
          componentLimitExceeded = true;
          overflow();
          return null;
        }
        if (!remainingBlocks) {
          overflow();
          return null;
        }
        remainingBlocks--;
        remainingComponents -= cost;
        const children = (
          values: readonly MarkdownRawBlock[],
          listItem = false,
        ) => {
          const result: MarkdownPreviewBlock[] = [];
          for (const value of values) {
            const child = adapt(value, true, listItem);
            if (child) result.push(child);
            if (componentLimitExceeded) break;
            if (!remainingBlocks) {
              if (value !== values.at(-1)) overflow();
              break;
            }
          }
          return result;
        };
        if (raw.kind === "quote")
          return { ...raw, children: children(raw.children) };
        if (raw.kind === "list") {
          const items: Extract<
            MarkdownPreviewBlock,
            { kind: "list" }
          >["items"][number][] = [];
          for (const item of raw.items) {
            if (componentLimitExceeded) break;
            if (!remainingItems) {
              overflow();
              break;
            }
            const retained = children(item.children, true);
            // Discard markup-only paragraphs, but preserve their child containers
            // and genuinely empty markers in their original owner.
            if (item.children.length && !retained.length) continue;
            remainingItems--;
            items.push({ ...item, children: retained });
          }
          return { ...raw, items };
        }
        if (raw.kind === "table")
          return {
            ...raw,
            rows: raw.rows.map((row, rowIndex) =>
              row.map((cell, column) =>
                inline(
                  cell,
                  nested ? slot++ : rowIndex * raw.align.length + column,
                ),
              ),
            ),
          };
        if (raw.kind === "code") return { ...raw, text: literal(raw.text) };
        if (raw.kind === "rule") return raw;
        const text = inline(raw.text, nested ? slot : undefined);
        if (raw.kind === "paragraph" && !text) {
          remainingBlocks++;
          remainingComponents += cost;
          return null;
        }
        if (nested) slot++;
        return { ...raw, text };
      };
      const block = adapt(raw, false);
      if (!block) continue;
      blocks.push(block);
      starts.push(sourceOffset + offsets[container.token.map?.[0] ?? 0]);
      truncated.push(document.truncated);
      if (componentLimitExceeded) break containers;
    }
  }
  // Keep parser overflow out of per-block prefix snapshots: a later streaming
  // suffix may replace the truncated tail while retaining the earlier blocks.
  document.truncated ||= tokenLimitExceeded;
}
