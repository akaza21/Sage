import { useWorkbenchStore } from "../../app/store";

export function clearChatSignInError(boundChatId?: string | null) {
  const workbench = useWorkbenchStore.getState();
  if (!boundChatId) {
    workbench.clearActiveChatError();
    return;
  }
  const ownership = workbench.captureChatOwnership(boundChatId);
  if (!ownership) return;
  workbench.setChatActionState(ownership, {
    sending: workbench.chatUiById[boundChatId]?.sending ?? false,
    error: null,
    errorKind: null,
  });
}
