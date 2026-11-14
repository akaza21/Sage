import type { StoreApi } from "zustand";
import type { ChatOwnershipIntent } from "../domain/chatOwnership";
import {
  editedChatEntry,
  emptyChatOwnershipEntry,
} from "./chatOwnershipProjection";
import type { WorkbenchState } from "./store";

type Selection = { harnessId: string | null; modelId: string | null };

export interface ChatChoiceActions {
  /** A selection the app derives for the chat; it is not a user edit. */
  setChatHarnessSelection: (
    intent: ChatOwnershipIntent,
    selection: Selection,
  ) => boolean;
  /** The user's harness or model choice in the chat. */
  chooseChatSelection: (
    intent: ChatOwnershipIntent,
    selection: Selection,
  ) => boolean;
  /** The user's access or effort choice, held in its own draft store. */
  chooseChatDraft: (intent: ChatOwnershipIntent, write: () => void) => boolean;
}

export function createChatChoiceActions(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  ownsChat: (state: WorkbenchState, intent: ChatOwnershipIntent) => boolean,
): ChatChoiceActions {
  const write = (
    intent: ChatOwnershipIntent,
    edit: boolean,
    selection?: Selection,
  ) => {
    const state = get();
    if (!ownsChat(state, intent)) return false;
    const entry = state.chatUiById[intent.chatId] ?? emptyChatOwnershipEntry();
    set({
      chatUiById: {
        ...state.chatUiById,
        [intent.chatId]: {
          ...(edit ? editedChatEntry(entry) : entry),
          ...selection,
        },
      },
    });
    return true;
  };
  return {
    setChatHarnessSelection: (intent, selection) =>
      write(intent, false, selection),
    chooseChatSelection: (intent, selection) => write(intent, true, selection),
    chooseChatDraft: (intent, writeDraft) => {
      if (!write(intent, true)) return false;
      writeDraft();
      return true;
    },
  };
}
