export const MAX_CHAT_TITLE_BYTES = 128;
const CHAT_TITLE_PREFIX = "Chat: ";
const CONTROL_CHARACTER = /\p{Cc}/u;

export function canonicalChatName(input: string): string {
  const title = input.trim();
  if (!title) {
    throw new Error("Chat title must not be empty after trimming");
  }
  if (new TextEncoder().encode(title).byteLength > MAX_CHAT_TITLE_BYTES) {
    throw new Error(
      `Chat title may contain at most ${MAX_CHAT_TITLE_BYTES} UTF-8 bytes`,
    );
  }
  if (CONTROL_CHARACTER.test(title)) {
    throw new Error("Chat title must not contain control characters");
  }
  return title;
}

export function canonicalChatTitle(input: string): string {
  return `${CHAT_TITLE_PREFIX}${canonicalChatName(input)}`;
}

export function chatDisplayTitle(title: string): string {
  return title.startsWith(CHAT_TITLE_PREFIX)
    ? title.slice(CHAT_TITLE_PREFIX.length)
    : title;
}
