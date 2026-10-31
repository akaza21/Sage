import type { AcpItem, TurnProjection } from "../../domain/acp";
import {
  renderMarkdownPreview,
  MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
  type InlineFormat,
} from "../files/markdownPreview";
import { terminalTurnStates } from "./acp/statusSemantics";

type TextItem = Extract<AcpItem, { kind: "text" }>;

/** One rendered markdown block (or list item / table cell), in DOM order. */
export interface ChatTextSegment {
  text: string;
  bullet: boolean;
}

export interface ChatTextMessage {
  sourceChars: number;
  itemId: string;
  role: "user" | "assistant";
  segments: readonly ChatTextSegment[];
  /** The renderer stopped early, so later source text is not shown. */
  previewLimited: boolean;
}

// Keyed by the immutable projection Turn, so a streaming update recomputes
// only the Turn that changed and dropped Turns are collected with it.
const cache = new WeakMap<TurnProjection, readonly ChatTextMessage[]>();
const expandedCache = new WeakMap<TurnProjection, readonly ChatTextMessage[]>();

function flatten(spans: readonly InlineFormat[]): string {
  return spans
    .map((span) =>
      typeof span === "string"
        ? span
        : "link" in span
          ? span.label
          : flatten(span.children),
    )
    .join("");
}

/**
 * Plain text of the blocks ChatMarkdown renders, in the same order as the
 * `p, h1-h6, blockquote, li, th, td, pre > code` elements it produces. Text
 * past the renderer's bounds is not shown, so it is not searchable either.
 */
function segments(source: string) {
  const document = renderMarkdownPreview(source);
  const formats = new Map(
    document.formats.map((format) => [
      `${format.blockIndex}:${format.itemIndex}`,
      format.spans,
    ]),
  );
  const inline = (text: string, block: number, item?: number) => {
    const spans = formats.get(`${block}:${item}`);
    return spans?.length ? flatten(spans) : text;
  };
  const values = document.blocks.flatMap<ChatTextSegment>((block, index) => {
    if (block.kind === "code")
      return block.language.toLowerCase() === "mermaid"
        ? []
        : [{ text: block.text, bullet: false }];
    if (block.kind === "list")
      return block.items.map((text, item) => ({
        text: inline(text, index, item),
        bullet: true,
      }));
    if (block.kind === "table")
      return block.rows.flatMap((row, rowIndex) =>
        row.map((text, column) => ({
          text: inline(text, index, rowIndex * block.align.length + column),
          bullet: false,
        })),
      );
    return [{ text: inline(block.text, index), bullet: false }];
  });
  return {
    segments: values,
    previewLimited: document.truncated || document.sourceLimitExceeded,
  };
}

/** The user prompt(s) and the presented final answer of one Turn. */
export function chatTurnText(
  turn: TurnProjection,
  expanded = false,
): readonly ChatTextMessage[] {
  const projectionCache = expanded ? expandedCache : cache;
  const cached = projectionCache.get(turn);
  if (cached) return cached;
  const primary = turn.items.filter(
    (item): item is TextItem =>
      item.kind === "text" && item.origin === "primary",
  );
  const answer = terminalTurnStates.has(turn.state)
    ? primary
        .filter(
          (item) =>
            item.payload.role === "assistant" && item.payload.text.trim(),
        )
        .sort(
          (left, right) =>
            (Number.isFinite(left.ordinal) ? left.ordinal : Infinity) -
            (Number.isFinite(right.ordinal) ? right.ordinal : Infinity),
        )
        .at(-1)
    : undefined;
  const messages = primary
    .filter(
      (item) =>
        item.payload.role === "user" ||
        (item.payload.role === "assistant" &&
          (item === answer || expanded || !terminalTurnStates.has(turn.state))),
    )
    .map((item) => {
      let rendered: ReturnType<typeof segments> | undefined;
      const read = () => (rendered ??= segments(item.payload.text));
      return {
        itemId: item.id,
        role: item.payload.role as "user" | "assistant",
        sourceChars: Math.min(
          item.payload.text.length,
          MAX_MARKDOWN_PREVIEW_SOURCE_BYTES,
        ),
        get segments() {
          return read().segments;
        },
        get previewLimited() {
          return read().previewLimited;
        },
      };
    });
  projectionCache.set(turn, messages);
  return messages;
}
