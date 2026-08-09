export const MAX_CHAT_TITLE_BYTES = 128;
const CHAT_TITLE_PREFIX = "Chat: ";
const CONTROL_CHARACTER = /\p{Cc}/u;

function isEdgeTrimCodePoint(value: number) {
  return (
    (value >= 0x0009 && value <= 0x000d) ||
    value === 0x0020 ||
    value === 0x0085 ||
    value === 0x00a0 ||
    value === 0x1680 ||
    (value >= 0x2000 && value <= 0x200a) ||
    value === 0x2028 ||
    value === 0x2029 ||
    value === 0x202f ||
    value === 0x205f ||
    value === 0x3000 ||
    value === 0xfeff
  );
}

function trimChatTitleEdges(input: string) {
  let start = 0;
  let end = input.length;
  while (start < end && isEdgeTrimCodePoint(input.charCodeAt(start)))
    start += 1;
  while (end > start && isEdgeTrimCodePoint(input.charCodeAt(end - 1)))
    end -= 1;
  return input.slice(start, end);
}

export function canonicalChatName(input: string): string {
  const title = trimChatTitleEdges(input);
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
