import { createContext } from "react";
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
  ((chrome: AgentSessionChrome | null) => void) | null
>(null);

interface AgentChatOwnerStore {
  ownerId: string | null;
}

const useAgentChatOwnerStore = create<AgentChatOwnerStore>(() => ({
  ownerId: null,
}));

export function pinAgentChatOwner(chatId: string | null): void {
  if (useAgentChatOwnerStore.getState().ownerId === chatId) return;
  useAgentChatOwnerStore.setState({ ownerId: chatId });
}

export function pinnedAgentChatOwner(): string | null {
  return useAgentChatOwnerStore.getState().ownerId;
}

export function usePinnedAgentChatOwner(): string | null {
  return useAgentChatOwnerStore((state) => state.ownerId);
}

export function clearAgentChatOwner(): void {
  pinAgentChatOwner(null);
}
