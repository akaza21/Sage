import type { MarkdownPreviewDocument } from "../../files/markdownPreview";

/** Oldest-first lengths of appended runs forming a contiguous suffix of a block/item/cell. */
export type StreamRuns = {
  blockIndex: number;
  itemIndex: number | undefined;
  runs: number[];
};

/** Retain spans for 16 chunks: at >19 ms/chunk this outlasts the 300 ms fade, so unwrapped runs are solid. */
export const MAX_STREAM_RUNS = 16;

/** Trail only appended rendered text, not raw Markdown or rewritten text. New blocks/items/rows fade whole; code never trails (s5/f0309). */
export default function nextStreamRuns(
  current: StreamRuns | null | undefined,
  previous: MarkdownPreviewDocument | null,
  next: MarkdownPreviewDocument,
): StreamRuns | null {
  const blockIndex = next.blocks.length - 1;
  const block = next.blocks[blockIndex];
  const before = previous?.blocks[blockIndex];
  if (
    !before ||
    previous.blocks.length !== blockIndex + 1 ||
    before.kind !== block.kind ||
    block.kind === "code"
  )
    return null;
  let itemIndex: number | undefined;
  let prior: string;
  let text: string;
  if (block.kind === "list") {
    if ((before as typeof block).items.length !== block.items.length)
      return null;
    itemIndex = block.items.length - 1;
    prior = (before as typeof block).items[itemIndex] ?? "";
    text = block.items[itemIndex] ?? "";
  } else if (block.kind === "table") {
    const row = block.rows.length - 1;
    if ((before as typeof block).rows.length - 1 !== row || row < 1)
      return null;
    itemIndex = row * block.align.length + block.rows[row].length - 1;
    prior = (before as typeof block).rows[row].at(-1) ?? "";
    text = block.rows[row].at(-1) ?? "";
  } else {
    prior = (before as typeof block).text;
    text = block.text;
  }
  if (text === prior || !text.startsWith(prior)) return null;
  const chars = text.length - prior.length;
  return {
    blockIndex,
    itemIndex,
    runs:
      current?.blockIndex === blockIndex &&
      current.itemIndex === itemIndex &&
      (next.formats
        .find(
          (format) =>
            format.blockIndex === blockIndex && format.itemIndex === itemIndex,
        )
        ?.spans.at(-1) ?? text) === text &&
      current.runs.reduce((sum, length) => sum + length, 0) <= prior.length
        ? [...current.runs.slice(1 - MAX_STREAM_RUNS), chars]
        : [chars],
  };
}
