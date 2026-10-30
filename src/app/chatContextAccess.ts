import type { StoreApi } from "zustand";
import type { ChatOwnershipIntent } from "../domain/chatOwnership";
import type { WorkbenchState } from "./store";

/** The owned state access shared by draft-context actions. */
export interface ChatContextAccess {
  get: StoreApi<WorkbenchState>["getState"];
  set: StoreApi<WorkbenchState>["setState"];
  ownsChat: (state: WorkbenchState, intent: ChatOwnershipIntent) => boolean;
  activeChatId: (state: WorkbenchState) => string | null;
}
