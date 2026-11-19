import type { StateCreator } from "zustand";

import type { SageTurnId } from "../domain/acp";
import type { ChatOwnershipIntent } from "../domain/chatOwnership";
import { emptyChatOwnershipEntry } from "./chatOwnershipProjection";
import { ownsChat } from "./chatOwnershipSlice";
import type { WorkbenchState } from "./store";

export interface ChatAuthSlice {
  dismissChatAuthTurn: (
    intent: ChatOwnershipIntent,
    turnId: SageTurnId,
  ) => boolean;
}

export const createChatAuthSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatAuthSlice
> = (set, get) => ({
  dismissChatAuthTurn: (intent, turnId) => {
    const state = get();
    if (!ownsChat(state, intent)) return false;
    const entry = state.chatUiById[intent.chatId] ?? emptyChatOwnershipEntry();
    set({
      chatUiById: {
        ...state.chatUiById,
        [intent.chatId]: { ...entry, dismissedAuthTurnId: turnId },
      },
    });
    return true;
  },
});
