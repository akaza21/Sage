import type { StateCreator } from "zustand";

import {
  validateChatHandoffDraft,
  type ChatHandoffIntent,
} from "../domain/chatHandoff";
import { matchesProjectIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

export interface ChatHandoffSlice {
  handoffChat: (intent: ChatHandoffIntent) => Promise<boolean>;
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function matchesHandoffSource(
  state: WorkbenchState,
  intent: ChatHandoffIntent,
) {
  return (
    matchesProjectIntent(state, intent.projectGeneration, intent.projectId) &&
    state.nativeTransitionEpoch === intent.transitionEpoch &&
    state.activeTabId === intent.sourceChatId &&
    state.tabs.some(
      (tab) => tab.id === intent.sourceChatId && tab.kind === "chat",
    ) &&
    state.tabs.some(
      (tab) => tab.id === intent.targetChatId && tab.kind === "chat",
    ) &&
    intent.sourceChatId !== intent.targetChatId
  );
}

export const createChatHandoffSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatHandoffSlice
> = (set, get) => ({
  handoffChat: async (intent) => {
    let draft: string;
    try {
      draft = validateChatHandoffDraft(intent.draft);
    } catch (error) {
      set({ error: errorMessage(error) });
      return false;
    }
    const source = get();
    if (
      source.projectTransitioning ||
      source.sending ||
      !Number.isSafeInteger(intent.projectGeneration) ||
      !Number.isSafeInteger(intent.transitionEpoch) ||
      intent.projectGeneration < 0 ||
      intent.projectGeneration >= Number.MAX_SAFE_INTEGER ||
      intent.transitionEpoch < 0 ||
      intent.transitionEpoch >= Number.MAX_SAFE_INTEGER ||
      !matchesHandoffSource(source, intent)
    ) {
      set({ error: "Chat Handoff intent is stale or invalid" });
      return false;
    }
    const sourceComposer = source.composerText;
    set({ projectTransitioning: true, error: null });
    try {
      await get().flushActiveEditor();
    } catch (error) {
      set({
        projectTransitioning: false,
        error: `Chat Handoff blocked by editor snapshot failure: ${errorMessage(error)}`,
      });
      return false;
    }
    if (!matchesHandoffSource(get(), intent)) {
      set({
        projectTransitioning: false,
        error: "Chat Handoff source or target changed before navigation",
      });
      return false;
    }

    set({ projectTransitioning: false });
    try {
      await get().activateChat(intent.targetChatId);
    } catch (error) {
      const current = get();
      set({
        projectTransitioning: false,
        composerText:
          current.project?.id === intent.projectId &&
          current.activeTabId === intent.sourceChatId
            ? sourceComposer
            : current.composerText,
        error: `Chat Handoff navigation failed: ${errorMessage(error)}`,
      });
      return false;
    }
    const current = get();
    const committed =
      current.phase === "ready" &&
      !current.projectTransitioning &&
      current.error === null &&
      current.project?.id === intent.projectId &&
      current.projectGeneration === intent.projectGeneration + 1 &&
      current.nativeTransitionEpoch === intent.transitionEpoch + 1 &&
      current.activeTabId === intent.targetChatId &&
      current.tabs.some(
        (tab) => tab.id === intent.targetChatId && tab.kind === "chat",
      );
    if (committed) {
      const targetOwner = current.captureChatOwnership(intent.targetChatId);
      if (targetOwner && current.setChatComposer(targetOwner, draft)) {
        return true;
      }
      set({ error: "Chat Handoff target ownership changed after navigation" });
      return false;
    }
    if (
      current.project?.id === intent.projectId &&
      current.activeTabId === intent.sourceChatId
    ) {
      set({ composerText: sourceComposer });
    }
    return false;
  },
});
