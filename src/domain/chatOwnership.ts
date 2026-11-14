import type { AttachedContext } from "./attachedContext";
import type { AcpCommandFailure } from "./acp";

export const MAX_CHAT_COMPOSER_BYTES = 16 * 1024;
export const MAX_CHAT_ERROR_BYTES = 2 * 1024;
export const MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES = 64;
export const MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES = 64;
export const MAX_CHAT_OWNERSHIP_ENTRIES =
  MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES + MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES;
export const MAX_QUEUED_CHAT_PROMPTS = 8;

export interface QueuedChatPrompt {
  id: string;
  text: string;
  attachedContext: readonly AttachedContext[];
  bindingId: string;
  modelId: string | null;
  status:
    "saving" | "queued" | "sending" | "blocked" | "outcomeUnknown" | "deleting";
}

export interface ChatTurnOrder {
  sessionOrdinal: number;
  turnOrdinal: number;
}

export interface ChatOwnershipEntry {
  composerText: string;
  /**
   * Advances on every edit of the composer or of the chat's setup choices, so
   * deferred work can tell whether the chat was edited since it looked.
   */
  editRevision?: number;
  attachedContext: readonly AttachedContext[];
  queuedPrompts?: readonly QueuedChatPrompt[];
  knownTurnOrder?: ChatTurnOrder | null;
  queuedTurnBarrier?: {
    afterOrder: ChatTurnOrder | null;
    deadlineMs: number;
  } | null;
  harnessId?: string | null;
  modelId?: string | null;
  sending: boolean;
  error: string | null;
  errorKind: AcpCommandFailure["kind"] | null;
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
