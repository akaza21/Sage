import type { TurnProjection } from "../../domain/acp";
import { chatTurnText } from "./chatTurnText";

export const MAX_CHAT_FIND_MATCHES = 500;
export const MAX_CHAT_FIND_SCAN_CHARS = 4_000_000;

export interface ChatMatch {
  key: string;
  turnId: string;
  itemId: string;
  segment: number;
  /** UTF-16 offsets into the rendered segment text. */
  start: number;
  end: number;
}

export interface ChatFindResult {
  matches: ChatMatch[];
  capped: boolean;
  scanLimited: boolean;
  previewLimited: boolean;
}

/**
 * Maps offsets in `text.toLowerCase()` back to `text`. Only built when case
 * folding changed the length (for example "İ"), so ASCII text costs nothing.
 */
function foldedOffsets(text: string): number[] {
  const offsets: number[] = [];
  let position = 0;
  for (const character of text) {
    const folded = character.toLowerCase().length;
    for (let unit = 0; unit < folded; unit++) offsets.push(position);
    position += character.length;
  }
  offsets.push(text.length);
  return offsets;
}

export function findChatMatches(
  turns: readonly TurnProjection[],
  query: string,
): ChatFindResult {
  const result: ChatFindResult = {
    matches: [],
    capped: false,
    scanLimited: false,
    previewLimited: false,
  };
  const needle = query.toLowerCase();
  if (!needle) return result;
  let budget = MAX_CHAT_FIND_SCAN_CHARS;
  for (const turn of turns) {
    for (const message of chatTurnText(turn)) {
      if (message.previewLimited) result.previewLimited = true;
      for (const [segment, { text }] of message.segments.entries()) {
        if (budget < text.length) {
          result.scanLimited = true;
          return result;
        }
        budget -= text.length;
        const folded = text.toLowerCase();
        const offsets =
          folded.length === text.length ? null : foldedOffsets(text);
        for (
          let start = folded.indexOf(needle);
          start >= 0;
          start = folded.indexOf(needle, start + needle.length)
        ) {
          if (result.matches.length === MAX_CHAT_FIND_MATCHES) {
            result.capped = true;
            return result;
          }
          const from = offsets ? offsets[start]! : start;
          const to = offsets
            ? offsets[start + needle.length]!
            : start + needle.length;
          result.matches.push({
            key: `${turn.id}\u0000${message.itemId}\u0000${segment}\u0000${from}`,
            turnId: turn.id,
            itemId: message.itemId,
            segment,
            start: from,
            end: Math.max(to, from + 1),
          });
        }
      }
    }
  }
  return result;
}

export const MAX_CHAT_FIND_QUERY = 1024;

export function chatFindStatus({
  pending,
  index,
  count,
  capped,
}: {
  pending: boolean;
  index: number;
  count: number;
  capped: boolean;
}) {
  if (pending || !count) return "0 results";
  if (capped) return `${index + 1} / ${MAX_CHAT_FIND_MATCHES}+ results`;
  if (count === 1) return "1 result";
  return `${index + 1} / ${count} results`;
}

export function stepChatMatch(index: number, direction: number, count: number) {
  return count ? (index + direction + count) % count : 0;
}
