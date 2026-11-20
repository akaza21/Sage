import { useWorkbenchStore } from "../../app/store";
import type { ChatOwnershipEntry } from "../../domain/chatOwnership";

export function clearChatSignInError(
  boundChatId?: string | null,
  expected?: ChatOwnershipEntry,
) {
  const workbench = useWorkbenchStore.getState();
  if (expected && workbench.chatUiById[boundChatId ?? ""] !== expected) return;
  const ownership = workbench.captureChatOwnership(boundChatId ?? undefined);
  if (!ownership) return;
  workbench.setChatActionState(ownership, {
    sending: workbench.chatUiById[ownership.chatId]?.sending ?? false,
    error: null,
    errorKind: null,
  });
}
