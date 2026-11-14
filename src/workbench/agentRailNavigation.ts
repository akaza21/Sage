import { useWorkbenchStore } from "../app/store";
import { createContext } from "react";

export const agentRailSectionSelectors = {
  Recent: '[aria-label="Recents"]',
  // In one list there is no Projects section, so Recents stands in.
  Projects: '[aria-label="Projects"], [aria-label="Recents"]',
} as const;

export function navigateAgentHome() {
  const state = useWorkbenchStore.getState();
  if (!state.project) return;
  void state.openNewChatDraft();
  document
    .querySelector<HTMLTextAreaElement>("[data-new-chat-draft] textarea")
    ?.focus();
}
export const AgentHomeNavigationContext = createContext(navigateAgentHome);
