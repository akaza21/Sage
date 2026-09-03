import { create } from "zustand";

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
