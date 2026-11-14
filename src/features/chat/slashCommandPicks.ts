const picks = new Map<string, string>();

export function setPickedCommand(chatId: string, name: string | null) {
  if (name) picks.set(chatId, name);
  else picks.delete(chatId);
}

export function pickedCommandFor(chatId: string): string | undefined {
  return picks.get(chatId);
}
