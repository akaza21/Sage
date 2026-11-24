import { useWorkbenchStore } from "../../app/store";

/**
 * Per chat, the offer text auto-filled into its empty field and not yet
 * edited, so a failed send's restore (from the composer or the queue)
 * replaces it. Entries outlive the composer chrome, which a session may
 * unmount, and are dropped with their chat: at most one per open chat.
 */
const filled = new Map<string, string>();

const fieldText = (chatId: string) =>
  useWorkbenchStore.getState().chatUiById[chatId]?.composerText;

/** Records `text` only when the chat's field holds it after the write. */
export function record(chatId: string, text: string) {
  const { tabs } = useWorkbenchStore.getState();
  for (const id of filled.keys())
    if (!tabs.some((tab) => tab.id === id)) filled.delete(id);
  if (fieldText(chatId) === text) filled.set(chatId, text);
}

export function forget(chatId: string) {
  filled.delete(chatId);
}

/** Whether `text` is the chat's untouched auto-fill. */
export const untouched = (chatId: string, text: string) =>
  filled.get(chatId) === text;

/**
 * Takes the chat's entry; returns its text only when the field still holds
 * it untouched, so the caller may replace it.
 */
export function take(chatId: string, currentText = fieldText(chatId)) {
  const text = filled.get(chatId);
  filled.delete(chatId);
  return text !== undefined && text === currentText ? text : undefined;
}
