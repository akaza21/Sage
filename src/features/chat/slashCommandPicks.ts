/**
 * The command each Chat's composer holds from a `/` menu pick, while its
 * text still names it. The lazy menu writes it and the lazy send route reads
 * it, so the first-Chat path carries neither.
 */
const picks = new Map<string, string>();

export function setPickedCommand(chatId: string, name: string | null) {
  if (name) picks.set(chatId, name);
  else picks.delete(chatId);
}

export function pickedCommandFor(chatId: string): string | undefined {
  return picks.get(chatId);
}
