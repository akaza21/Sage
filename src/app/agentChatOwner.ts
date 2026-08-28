let ownerId: string | null = null;

export function pinAgentChatOwner(chatId: string | null): void {
  ownerId = chatId;
}

export function pinnedAgentChatOwner(): string | null {
  return ownerId;
}

export function clearAgentChatOwner(): void {
  ownerId = null;
}
