import type { StoreApi } from "zustand";
import {
  boundedChatError,
  validateChatComposerText,
} from "../domain/chatOwnership";
import { openNewChatDraft, selectDraftHarness } from "./newChatDraftNavigation";
import type { NewChatDraft, NewChatDraftSlice } from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";

/** Loaded before the first draft opens; its mutations then run synchronously. */
export function createNewChatDraftActions(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
): Pick<
  NewChatDraftSlice,
  | "openNewChatDraft"
  | "updateNewChatDraft"
  | "selectDraftHarness"
  | "selectDraftModel"
  | "sendNewChatDraft"
  | "reloadNewChatDraft"
> {
  let selectionRequest: object | undefined;
  const submit = async (draft: NewChatDraft) => {
    const submission = {
      projectId: draft.projectId,
      chatId: null,
      ownershipEpoch: get().chatOwnershipEpoch,
    };
    set({
      newChatDraft: { ...draft, creating: true, error: null },
      pendingFirstSend: submission,
    });
    const { runNewChatDraftSend } = await import("./runNewChatDraftSend");
    await runNewChatDraftSend(set, get, draft, submission);
  };
  return {
    openNewChatDraft: async (projectId) => {
      if (!get().newChatDraft) selectionRequest = undefined;
      return openNewChatDraft(set, get, projectId);
    },
    selectDraftHarness: (harnessId) => {
      const request = {};
      selectionRequest = request;
      return selectDraftHarness(
        set,
        get,
        harnessId,
        () => selectionRequest === request,
      );
    },
    updateNewChatDraft: (patch) => {
      const draft = get().newChatDraft;
      if (!draft || draft.creating) return;
      try {
        if (patch.text !== undefined) validateChatComposerText(patch.text);
        set({ newChatDraft: { ...draft, ...patch, error: null } });
      } catch (error) {
        set({
          newChatDraft: { ...draft, error: boundedChatError(String(error)) },
        });
      }
    },
    selectDraftModel: (modelId) => {
      const state = get();
      const draft = state.newChatDraft;
      if (
        !draft ||
        draft.creating ||
        state.selectedHarnessId !== draft.harnessId
      )
        return;
      const model = state.models.find((model) => model.id === modelId);
      if (!model) return;
      selectionRequest = undefined;
      set({
        newChatDraft: {
          ...draft,
          modelId,
          effort:
            draft.effort && model.effortLevels?.includes(draft.effort)
              ? draft.effort
              : null,
        },
      });
    },
    sendNewChatDraft: async () => {
      const draft = get().newChatDraft;
      if (
        !draft ||
        draft.creating ||
        !draft.text.trim() ||
        get().projectTransitioning
      )
        return;
      await submit(draft);
    },
    reloadNewChatDraft: async () => {
      const state = get();
      const draft = state.newChatDraft;
      if (
        !draft?.reconcileCreation ||
        state.project?.id !== draft.projectId ||
        state.projectTransitioning ||
        state.pendingFirstSend
      )
        return;
      await submit(draft);
    },
  };
}
