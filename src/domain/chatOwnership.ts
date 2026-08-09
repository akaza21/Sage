export const MAX_CHAT_COMPOSER_BYTES = 16 * 1024;
export const MAX_CHAT_ERROR_BYTES = 2 * 1024;
export const MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES = 64;
export const MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES = 64;
export const MAX_CHAT_OWNERSHIP_ENTRIES =
  MAX_ATTACHED_CHAT_OWNERSHIP_ENTRIES + MAX_ARCHIVED_CHAT_OWNERSHIP_ENTRIES;

export interface ChatOwnershipEntry {
  composerText: string;
  sending: boolean;
  error: string | null;
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
  if (new TextEncoder().encode(text).byteLength > MAX_CHAT_COMPOSER_BYTES) {
    throw new Error(`Chat composer bytes exceed ${MAX_CHAT_COMPOSER_BYTES}`);
  }
  return text;
}

export function boundedChatError(message: string) {
  const encoder = new TextEncoder();
  let bytes = 0;
  let bounded = "";
  for (const character of message) {
    const characterBytes = encoder.encode(character).byteLength;
    if (bytes + characterBytes > MAX_CHAT_ERROR_BYTES) break;
    bounded += character;
    bytes += characterBytes;
  }
  return bounded;
}
