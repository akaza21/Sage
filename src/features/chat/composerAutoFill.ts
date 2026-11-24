import { useWorkbenchStore } from "../../app/store";
import { autoFilled } from "./acp/useAcpChatActions";

/*
 * The composer chrome's side of the auto-fill record. The record itself stays
 * with the send path (`useAcpChatActions`), whose failed restore replaces an
 * untouched fill, so it outlives the chrome, which a session may unmount.
 */

/**
 * Records the fill only once the chat's field holds exactly `text`; a refused
 * write records nothing. Entries of chats that closed are dropped.
 */
export function record(chatId: string, text: string) {
  const { tabs, chatUiById } = useWorkbenchStore.getState();
  for (const id of autoFilled.keys())
    if (!tabs.some((tab) => tab.id === id)) autoFilled.delete(id);
  if (chatUiById[chatId]?.composerText === text)
    autoFilled.set(chatId, text.trim());
}

export const forget = (chatId: string) => autoFilled.delete(chatId);

/** Whether the field's `text` is the chat's untouched fill. */
export const untouched = (chatId: string, text: string) =>
  autoFilled.get(chatId) === text.trim();
