import type { TurnProjection } from "../../domain/acp";
import { chatTurnText } from "./chatTurnText";

/** At most this many dashes, whatever the Turn count (bounded list rule). */
export const MAX_TURN_DASHES = 60;
export const DASH_PITCH = 9;
export const PROMPT_PREVIEW_CHARS = 120;
export const ANSWER_PREVIEW_CHARS = 240;

export function dashWidth(distance: number) {
  return [24, 18, 13, 9][Math.abs(distance)] ?? 6;
}

/**
 * Evenly samples `count` Turns down to `maximum` dashes, always keeping the
 * first, the last and the current Turn.
 */
export function sampleTurnIndices(
  count: number,
  current: number,
  maximum = MAX_TURN_DASHES,
): number[] {
  const limit = Math.max(3, Math.min(MAX_TURN_DASHES, Math.floor(maximum)));
  if (count <= limit) return Array.from({ length: count }, (_, index) => index);
  const selected = new Set(
    Array.from({ length: limit }, (_, sample) =>
      Math.round((sample * (count - 1)) / (limit - 1)),
    ),
  );
  const active = Math.max(0, Math.min(count - 1, current));
  if (!selected.has(active)) {
    let nearest = -1;
    for (const index of selected)
      if (
        index !== 0 &&
        index !== count - 1 &&
        (nearest < 0 || Math.abs(index - active) < Math.abs(nearest - active))
      )
        nearest = index;
    selected.delete(nearest);
    selected.add(active);
  }
  return [...selected].sort((left, right) => left - right);
}

export function truncatePreview(value: string, limit: number) {
  return value.length <= limit ? value : value.slice(0, limit - 1) + "…";
}

const prompts = new WeakMap<TurnProjection, string>();

/**
 * The Turn's user prompt on one line, bounded. Reads only the prompt items
 * (no markdown parse), so labelling 60 dashes stays cheap.
 */
export function turnPrompt(turn: TurnProjection): string {
  const cached = prompts.get(turn);
  if (cached !== undefined) return cached;
  let text = "";
  for (const item of turn.items) {
    if (item.kind !== "text" || item.origin !== "primary") continue;
    if (item.payload.role !== "user") continue;
    text +=
      (text ? " " : "") + item.payload.text.slice(0, 4 * PROMPT_PREVIEW_CHARS);
    if (text.length > 4 * PROMPT_PREVIEW_CHARS) break;
  }
  const value = truncatePreview(
    text.replace(/\s+/gu, " ").trim() || `Turn ${turn.ordinal}`,
    PROMPT_PREVIEW_CHARS,
  );
  prompts.set(turn, value);
  return value;
}

/** The start of the presented answer, plain text with "• " bullets. */
export function turnAnswerPreview(turn: TurnProjection): string {
  const answer = chatTurnText(turn).find(
    (message) => message.role === "assistant",
  );
  if (!answer) return "";
  let text = "";
  for (const segment of answer.segments) {
    text += (text ? "\n" : "") + (segment.bullet ? "• " : "") + segment.text;
    if (text.length > ANSWER_PREVIEW_CHARS) break;
  }
  return truncatePreview(text, ANSWER_PREVIEW_CHARS);
}
