import { useWorkbenchStore } from "../app/store";
import { createContext } from "react";

export function navigateAgentHome() {
  const state = useWorkbenchStore.getState();
  if (!state.project) return;
  void state.openNewChatDraft();
  document
    .querySelector<HTMLTextAreaElement>("[data-new-chat-draft] textarea")
    ?.focus();
}
export const AgentHomeNavigationContext = createContext(navigateAgentHome);
