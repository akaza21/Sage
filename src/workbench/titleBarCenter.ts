import { chatDisplayTitle } from "../domain/chatTitle";

const TITLE_BAR_LABEL_CAP = 48;

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
  if (name.length <= TITLE_BAR_LABEL_CAP) return name;
  return `${name.slice(0, TITLE_BAR_LABEL_CAP - 1)}…`;
}

export function titleBarFocusedChatId(
  chatIds: readonly string[],
  activeTabId: string | null,
): string | null {
  if (activeTabId && chatIds.includes(activeTabId)) return activeTabId;
  return chatIds[0] ?? null;
}
