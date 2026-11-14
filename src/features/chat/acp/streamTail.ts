import { markdownTextTail } from "../../files/markdownTextLocations";
import type { MarkdownPreviewDocument } from "../../files/markdownPreview";

export type StreamRuns = {
  blockIndex: number;
  itemIndex: number | undefined;
  runs: number[];
};

// 16 chunks at more than 19 ms each outlast the 300 ms fade, so unwrapped runs
// are solid.
export const MAX_STREAM_RUNS = 16;

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
    block.kind === "code" ||
    block.kind === "rule"
  )
    return null;
  const oldEnd = markdownTextTail(before);
  const nextEnd = markdownTextTail(block);
  if (oldEnd.count !== nextEnd.count) return null;
  const oldTail = oldEnd.tail;
  const tail = nextEnd.tail;
  if (
    !oldTail ||
    !tail ||
    oldTail.code ||
    tail.code ||
    oldTail.itemIndex !== tail.itemIndex
  )
    return null;
  const { text, itemIndex } = tail;
  const prior = oldTail.text;
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
