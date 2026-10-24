import type { AttachedContext } from "./attachedContext";
import { isRecord } from "./guards";
import { utf8ByteLength } from "./text";

/** Quote and comment bytes; equals MAX_ATTACHED_CONTEXT_NOTE_BYTES (Rust mirrors it). */
export const MAX_TRANSCRIPT_QUOTE_BYTES = 4096;

/** A selection inside one assistant answer; disposable WebView state until sent. */
export interface TranscriptQuote {
  turnId: string;
  itemId: string;
  text: string;
  startOffset: number;
  endOffset: number;
}

export interface TranscriptQuoteContext {
  kind: "transcriptQuote";
  /**
   * Wire field bounded 1-8. Display numbers and the prompt's "Annotation N"
   * follow the draft order, so deletions never need a renumbering pass.
   */
  ordinal: number;
  sourceTurnId: string;
  sourceItemId: string;
  text: string;
  comment?: string;
  startOffset: number;
  endOffset: number;
}

const KEYS = [
  "kind",
  "ordinal",
  "sourceTurnId",
  "sourceItemId",
  "text",
  "comment",
  "startOffset",
  "endOffset",
];
const bounded = (value: unknown, bytes: number): value is string =>
  typeof value === "string" &&
  utf8ByteLength(value) <= bytes &&
  !value.includes("\0");
const identifier = (value: unknown) =>
  bounded(value, 512) &&
  value !== "" &&
  ![...value].some((c) => c < " " || (c >= "\x7f" && c <= "\x9f"));
const offset = (value: unknown): value is number =>
  Number.isSafeInteger(value) && (value as number) >= 0;

/** Admits an exact-key, byte-bounded quote; returns the validated value itself. */
export function parseTranscriptQuoteContext(
  value: unknown,
): TranscriptQuoteContext | null {
  if (!isRecord(value) || value.kind !== "transcriptQuote") return null;
  const { ordinal, text, comment, startOffset, endOffset } = value;
  return Object.keys(value).every((key) => KEYS.includes(key)) &&
    Number.isInteger(ordinal) &&
    (ordinal as number) >= 1 &&
    (ordinal as number) <= 8 &&
    identifier(value.sourceTurnId) &&
    identifier(value.sourceItemId) &&
    bounded(text, MAX_TRANSCRIPT_QUOTE_BYTES) &&
    text.trim() &&
    (comment === undefined || bounded(comment, MAX_TRANSCRIPT_QUOTE_BYTES)) &&
    offset(startOffset) &&
    offset(endOffset) &&
    endOffset > startOffset
    ? (value as unknown as TranscriptQuoteContext)
    : null;
}

export const isTranscriptQuote = (
  context: AttachedContext,
): context is TranscriptQuoteContext => context.kind === "transcriptQuote";
