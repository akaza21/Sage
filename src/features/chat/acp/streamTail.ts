import type {
  MarkdownPreviewBlock,
  MarkdownPreviewDocument,
} from "../../files/markdownPreview";

/** Where a streamed answer's newest appended run sits, and how long it is. */
export type StreamTail = {
  blockIndex: number;
  itemIndex: number | undefined;
  chars: number;
  /** Changes with every chunk, so each new run mounts (and fades) afresh. */
  key: number;
};

/** The rendered text a block ends in: its last item or last cell. */
const tailText = (block: MarkdownPreviewBlock) =>
  (block.kind === "list"
    ? block.items.at(-1)
    : block.kind === "table"
      ? block.rows.at(-1)?.at(-1)
      : // Code never reaches here (streamTailTarget refuses it first).
        block.text) ?? "";

/**
 * The run to fade when the answer's last text grew (Codex s5/f0309: only the
 * newest words trail). It is measured in rendered text, never raw markdown:
 * stripped markers and collapsed whitespace don't count, and a chunk that
 * re-renders earlier text (a marker closing) has no run. A new block, list
 * item or table row fades whole on mount instead; code blocks never trail.
 */
export function streamTailTarget(
  previous: MarkdownPreviewDocument | null,
  next: MarkdownPreviewDocument,
  key: number,
): StreamTail | null {
  const blockIndex = next.blocks.length - 1;
  const block = next.blocks[blockIndex];
  const before = previous?.blocks[blockIndex];
  if (
    !block ||
    !before ||
    previous.blocks.length !== next.blocks.length ||
    before.kind !== block.kind ||
    block.kind === "code"
  )
    return null;
  let itemIndex: number | undefined;
  if (block.kind === "list" && before.kind === "list") {
    if (before.items.length !== block.items.length) return null;
    itemIndex = block.items.length - 1;
  } else if (block.kind === "table" && before.kind === "table") {
    const row = block.rows.length - 1;
    if (before.rows.length !== block.rows.length || row < 1) return null;
    itemIndex = row * block.align.length + block.rows[row].length - 1;
  }
  const prior = tailText(before);
  const text = tailText(block);
  const chars = text.length - prior.length;
  return chars > 0 && text.startsWith(prior)
    ? { blockIndex, itemIndex, chars, key }
    : null;
}
