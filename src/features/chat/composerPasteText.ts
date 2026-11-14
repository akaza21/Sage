import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import {
  PROMPT_ATTACHMENT_COUNT,
  PROMPT_ATTACHMENT_TOTAL_BYTES,
} from "../../ports/promptAttachments";
import type { HeldInput } from "./pastedTextTypes";

/** UTF-8 length without encoding a copy; a huge clipboard is never duplicated. */
export function utf8Length(value: string): number {
  let bytes = 0;
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x80) bytes += 1;
    else if (code < 0x800) bytes += 2;
    else if (
      code >= 0xd800 &&
      code <= 0xdbff &&
      (value.charCodeAt(index + 1) & 0xfc00) === 0xdc00
    ) {
      bytes += 4;
      index += 1;
    } else bytes += 3; // Includes a lone surrogate, encoded as U+FFFD.
  }
  return bytes;
}

export function formatSize(bytes: number): string {
  return `${Math.max(1, Math.ceil(bytes / 1024)).toLocaleString("en-US")} KB`;
}

export function lineCount(text: string): number {
  let lines = text.endsWith("\n") ? 0 : 1;
  for (let index = text.indexOf("\n"); index >= 0;) {
    lines += 1;
    index = text.indexOf("\n", index + 1);
  }
  return lines;
}

export function pastedTextLabel(bytes: number, lines: number): string {
  return `Pasted text · ${lines.toLocaleString("en-US")} ${lines === 1 ? "line" : "lines"} · ${formatSize(bytes)}`;
}

export function stagedItemLabel(item: {
  name: string;
  bytes: number;
  lines: number | null;
}): string {
  return item.lines === null
    ? `${item.name} · ${formatSize(item.bytes)}`
    : pastedTextLabel(item.bytes, item.lines);
}

const TYPED_LIMIT = formatSize(MAX_CHAT_COMPOSER_BYTES);
const ATTACHMENT_LIMIT = formatSize(PROMPT_ATTACHMENT_TOTAL_BYTES);

export function typedOverflowMessage(
  bytes: number,
  refusal: string | null,
): string {
  const held = `Messages hold up to ${TYPED_LIMIT} of typed text; this edit would make at least ${formatSize(bytes)}.`;
  return refusal
    ? `${held} ${refusal}`
    : `${held} Paste longer text to attach it instead.`;
}

export interface PasteAttachability {
  nativeAcp: boolean;
  /** The session the next send uses; null until the chat starts one. */
  session: { capabilities: { promptResources?: boolean } } | null | undefined;
}

export function pasteAttachmentRefusal({
  nativeAcp,
  session,
}: PasteAttachability): string | null {
  // Native admission treats an absent capability as unsupported.
  const acceptsResources = session
    ? session.capabilities.promptResources === true
    : null;
  if (!nativeAcp)
    return `Text over ${TYPED_LIMIT} is attached as a file, which needs an ACP harness in the desktop app.`;
  if (acceptsResources === false)
    return `This harness does not accept attached text (no embedded-context support), so text over ${TYPED_LIMIT} cannot be sent here.`;
  return null;
}

export function heldInputMessage(
  held: Exclude<HeldInput, { kind: "paste" | "pick" }>,
  refusal: string | null,
): string {
  switch (held.kind) {
    case "edit":
      return typedOverflowMessage(held.bytes, refusal);
    case "overLimit":
      return `Pasted text is at least ${formatSize(held.bytes)}, over the ${ATTACHMENT_LIMIT} attachment limit. Nothing was pasted. Save it to a file in the project and ask the agent to read it.`;
    case "pending":
      return "Another long paste was still being attached. Nothing was pasted; paste again once it appears.";
  }
}

export type PasteDecision =
  | { kind: "inline" }
  | { kind: "attach"; bytes: number }
  | { kind: "refuse"; message: string };

export function fitsComposer(before: string, pasted: string, after: string) {
  return (
    utf8Length(before) + utf8Length(pasted) + utf8Length(after) <=
    MAX_CHAT_COMPOSER_BYTES
  );
}

export function pasteDecision({
  before,
  pasted,
  after,
  staged,
  refusal,
}: {
  before: string;
  pasted: string;
  after: string;
  staged: { bytes: number; count: number };
  refusal: string | null;
}): PasteDecision {
  const bytes = utf8Length(pasted);
  if (fitsComposer(before, pasted, after)) return { kind: "inline" };
  const size = formatSize(bytes);
  if (refusal)
    return { kind: "refuse", message: `${refusal} Nothing was pasted.` };
  if (bytes > PROMPT_ATTACHMENT_TOTAL_BYTES)
    return {
      kind: "refuse",
      message: `Pasted text is ${size}, over the ${ATTACHMENT_LIMIT} attachment limit. Nothing was pasted. Save it to a file in the project and ask the agent to read it.`,
    };
  if (staged.count >= PROMPT_ATTACHMENT_COUNT)
    return {
      kind: "refuse",
      message: `A message carries at most ${PROMPT_ATTACHMENT_COUNT} attachments. Nothing was pasted. Remove an attachment first.`,
    };
  if (staged.bytes + bytes > PROMPT_ATTACHMENT_TOTAL_BYTES)
    return {
      kind: "refuse",
      message: `Pasted text is ${size}, but only ${formatSize(PROMPT_ATTACHMENT_TOTAL_BYTES - staged.bytes)} of the ${ATTACHMENT_LIMIT} attachment limit is left. Nothing was pasted. Remove an attachment first.`,
    };
  return { kind: "attach", bytes };
}
