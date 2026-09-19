import type { DiffLineCommentContext } from "./attachedContext";

export const MAX_CHAT_COMPOSER_BYTES = 16 * 1024;
export const MAX_CHAT_ERROR_BYTES = 2 * 1024;
export const MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES = 64;
export const MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES = 64;
export const MAX_CHAT_OWNERSHIP_ENTRIES =
  MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES + MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES;

export interface ChatOwnershipEntry {
  composerText: string;
  attachedContext: readonly DiffLineCommentContext[];
  sending: boolean;
  error: string | null;
  modeError?: string | null;
  modeErrorOwner?: { sessionId: string; runtimeEpoch: string } | null;
  modeChanging?: boolean;
  requestGeneration: number;
}

export interface ChatOwnershipIntent {
  projectId: string;
  ownershipEpoch: number;
  chatId: string;
  projectGeneration: number;
  transitionEpoch: number;
}

export function validateChatComposerText(text: string) {
  if (utf8ByteLength(text) > MAX_CHAT_COMPOSER_BYTES) {
    throw new Error(`Chat composer bytes exceed ${MAX_CHAT_COMPOSER_BYTES}`);
  }
  return text;
}

export function boundedChatError(message: string) {
  return truncateUtf8(message, MAX_CHAT_ERROR_BYTES);
}
import { truncateUtf8, utf8ByteLength } from "./text";
