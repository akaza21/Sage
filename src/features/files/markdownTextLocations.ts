import type { MarkdownPreviewBlock } from "./markdownPreview";

/** The renderer's inline slots, in depth-first DOM order within one root block. */
export function* markdownTextLocations(root: MarkdownPreviewBlock) {
  let slot = 0;
  function* visit(
    block: MarkdownPreviewBlock,
    nested: boolean,
    bullet: boolean,
  ): Generator<{
    text: string;
    itemIndex?: number;
    bullet: boolean;
    code?: true;
  }> {
    if (block.kind === "quote") {
      for (const child of block.children) yield* visit(child, true, bullet);
    } else if (block.kind === "list") {
      for (const item of block.items)
        for (const child of item.children) yield* visit(child, true, true);
    } else if (block.kind === "table") {
      for (const row of block.rows)
        for (const text of row)
          yield { text, itemIndex: slot++, bullet: false };
    } else if (block.kind === "code") {
      if (block.language.toLowerCase() !== "mermaid")
        yield { text: block.text, bullet: false, code: true };
    } else if (block.kind !== "rule")
      yield {
        text: block.text,
        itemIndex: nested ? slot++ : undefined,
        bullet,
      };
  }
  yield* visit(root, false, false);
}

/** Streaming keeps only the final location and count, without allocating a leaf array. */
export function markdownTextTail(block: MarkdownPreviewBlock) {
  let count = 0;
  let tail;
  for (const location of markdownTextLocations(block)) {
    tail = location;
    count++;
  }
  return { count, tail };
}

/** Include ordered lists in quotes and nested items when reserving marker space. */
export function markdownListMarkerWidth(
  blocks: readonly MarkdownPreviewBlock[],
): number {
  let digits = 1;
  for (const block of blocks) {
    if (block.kind === "quote")
      digits = Math.max(digits, markdownListMarkerWidth(block.children));
    if (block.kind === "list") {
      if (block.ordered)
        digits = Math.max(
          digits,
          String((block.start ?? 1) + block.items.length - 1).length,
        );
      for (const item of block.items)
        digits = Math.max(digits, markdownListMarkerWidth(item.children));
    }
  }
  return digits;
}
