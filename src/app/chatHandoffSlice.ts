import type { StateCreator } from "zustand";

import type { ChatHandoffIntent } from "../domain/chatHandoff";
import { errorMessage } from "../domain/errors";
import type { runChatHandoff } from "./runChatHandoff";
import type { WorkbenchState } from "./store";

export interface ChatHandoffSlice {
  handoffChat: (intent: ChatHandoffIntent) => Promise<boolean>;
}

export const createChatHandoffSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatHandoffSlice
> = (set, get) => ({
  // A handoff runs only once the user asks for one, so it loads then.
  handoffChat: async (intent) => {
    let run: typeof runChatHandoff;
    try {
      ({ runChatHandoff: run } = await import("./runChatHandoff"));
    } catch (error) {
      set({ error: `Chat Handoff failed: ${errorMessage(error)}` });
      return false;
    }
    return run(set, get, intent);
  },
});
