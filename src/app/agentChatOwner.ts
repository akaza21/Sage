import { createContext, useContext } from "react";
import { create } from "zustand";

import type { AcpChatPage } from "../domain/acp";
import type { ChatMessage } from "../domain/workbench";

export interface AgentSessionChrome {
  chatId: string;
  harnessStatus: string;
  canCloseSession: boolean;
  canEndSession: boolean;
  busy: boolean;
  acpPage: AcpChatPage | null;
  messages: readonly ChatMessage[];
  actions: {
    current: {
      closeSession: () => void;
      endSession: () => void;
      focusComposer: () => void;
    };
  };
}

export const AgentSessionChromeContext = createContext<
  ((chatId: string, chrome: AgentSessionChrome | null) => void) | null
>(null);

interface AgentChatOwnerStore {
  ownerId: string | null;
  agentFocusId: string | null;
}

const useAgentChatOwnerStore = create<AgentChatOwnerStore>(() => ({
  ownerId: null,
  agentFocusId: null,
}));

/**
 * Readers keep the last focused Chat while the body sleeps under Activity, so
 * hidden renders never see the withdrawn pin.
 */
export const AgentChatOwnerScopeContext = createContext(false);

export function pinAgentChatOwner(chatId: string | null): void {
  const { ownerId, agentFocusId } = useAgentChatOwnerStore.getState();
  if (ownerId === chatId && agentFocusId === chatId) return;
  useAgentChatOwnerStore.setState({ ownerId: chatId, agentFocusId: chatId });
}

export function releaseAgentChatOwner(): void {
  if (useAgentChatOwnerStore.getState().ownerId === null) return;
  useAgentChatOwnerStore.setState({ ownerId: null });
}

export function pinnedAgentChatOwner(): string | null {
  return useAgentChatOwnerStore.getState().ownerId;
}

export function usePinnedAgentChatOwner(): string | null {
  const insideAgent = useContext(AgentChatOwnerScopeContext);
  return useAgentChatOwnerStore((state) =>
    insideAgent ? state.agentFocusId : state.ownerId,
  );
}

export function agentChatFocus(): string | null {
  return useAgentChatOwnerStore.getState().agentFocusId;
}

export function useAgentChatFocus(): string | null {
  return useAgentChatOwnerStore((state) => state.agentFocusId);
}

export function clearAgentChatOwner(): void {
  pinAgentChatOwner(null);
}
