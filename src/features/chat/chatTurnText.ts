import type { AcpItem, TurnProjection } from "../../domain/acp";
import {
  renderMarkdownPreview,
  type InlineFormat,
} from "../files/markdownPreview";
import { markdownTextLocations } from "../files/markdownTextLocations";
import { terminalTurnStates } from "./acp/statusSemantics";

type TextItem = Extract<AcpItem, { kind: "text" }>;

export interface ChatTextSegment {
  text: string;
  bullet: boolean;
}

export interface ChatTextMessage {
  itemId: string;
  role: "user" | "assistant";
  segments: readonly ChatTextSegment[];
  previewLimited: boolean;
}

// Keyed by the immutable projection Turn, so a streaming update recomputes
// only the Turn that changed and dropped Turns are collected with it.
const cache = new WeakMap<TurnProjection, readonly ChatTextMessage[]>();

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

// Must stay in the same order as the `p, h1-h6, [data-markdown-inline], th, td,
// pre > code` elements ChatMarkdown produces.
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
    return [...markdownTextLocations(block)].map(
      ({ text, itemIndex, bullet, code }) => ({
        text: code ? text : inline(text, index, itemIndex),
        bullet,
      }),
    );
  });
  return {
    segments: values,
    previewLimited: document.truncated || document.sourceLimitExceeded,
  };
}

export function chatTurnText(turn: TurnProjection): readonly ChatTextMessage[] {
  const cached = cache.get(turn);
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
    .filter((item) => item.payload.role === "user" || item === answer)
    .map((item) => ({
      itemId: item.id,
      role: item.payload.role as "user" | "assistant",
      ...segments(item.payload.text),
    }));
  cache.set(turn, messages);
  return messages;
}
