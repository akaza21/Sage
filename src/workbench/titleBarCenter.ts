import { chatDisplayTitle } from "../domain/chatTitle";

function hasControlCharacter(value: string): boolean {
  return value.split("").some((character) => {
    const code = character.charCodeAt(0);
    return code < 32 || code === 127;
  });
}

export function titleBarSessionTitle(
  tabTitle: string | null | undefined,
): string | null {
  if (!tabTitle) return null;
  const name = chatDisplayTitle(tabTitle).trim();
  if (!name || hasControlCharacter(name)) return null;
  // The title bar ellipsizes in CSS, so the label fills the room it has.
  return name;
}

export function titleBarFocusedChatId(
  chatIds: readonly string[],
  activeTabId: string | null,
): string | null {
  if (activeTabId && chatIds.includes(activeTabId)) return activeTabId;
  return chatIds[0] ?? null;
}
