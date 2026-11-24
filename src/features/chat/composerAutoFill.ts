import { useWorkbenchStore } from "../../app/store";
import { autoFilled } from "./acp/useAcpChatActions";

/**
 * Records an auto-fill once the chat's field holds exactly `text`, and says
 * whether it did: a refused write (lost ownership) records nothing. The
 * record stays with the send path (`autoFilled`), whose failed restore
 * replaces an untouched fill, so it outlives the composer chrome, which a
 * session may unmount. Entries of chats that closed are dropped here, so
 * there is at most one per open chat.
 */
export function record(chatId: string, text: string) {
  const { tabs, chatUiById } = useWorkbenchStore.getState();
  for (const id of autoFilled.keys())
    if (!tabs.some((tab) => tab.id === id)) autoFilled.delete(id);
  return (
    chatUiById[chatId]?.composerText === text &&
    !!autoFilled.set(chatId, text.trim())
  );
}
