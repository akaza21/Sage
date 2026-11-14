import type { StoreApi } from "zustand";
import {
  boundedChatError,
  validateChatComposerText,
} from "../domain/chatOwnership";
import { reconcileDraftModel } from "./harnessSlice";
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
  const submit = async (draft: NewChatDraft) => {
    if (draft.submission.status === "submitting") return;
    const attempt = {
      status: "submitting" as const,
      creation: draft.submission,
    };
    const submittingDraft = { ...draft, submission: attempt, error: null };
    // A chat this draft already created is handed the prompt only if it is
    // not edited from here on; a chat created by this attempt starts unedited.
    const editRevision =
      draft.submission.status === "created"
        ? (get().chatUiById[draft.submission.chatId]?.editRevision ?? 0)
        : 0;
    const submission = {
      projectId: draft.projectId,
      chatId: null,
      ownershipEpoch: get().chatOwnershipEpoch,
    };
    set({
      newChatDraft: submittingDraft,
      pendingFirstSend: submission,
    });
    let released = {
      submission: attempt.creation,
      error: "",
    };
    try {
      // Every module the send needs loads before anything is created, so a
      // failed chunk load cannot strand the draft mid-creation.
      const [send] = await Promise.all([
        import("./runNewChatDraftSend"),
        import("./runCreateChat"),
      ]);
      released = await send.runNewChatDraftSend(
        set,
        get,
        submittingDraft,
        submission,
        editRevision,
      );
    } catch (error) {
      released.error = boundedChatError(String(error));
    } finally {
      const state = get();
      // Draft identity survives project ownership recovery. Only this attempt
      // may release it; navigation to another project must not resurrect it.
      if (
        state.newChatDraft?.id === draft.id &&
        state.newChatDraft.submission === attempt
      )
        set({
          newChatDraft: { ...state.newChatDraft, ...released },
          pendingFirstSend:
            state.pendingFirstSend === submission
              ? null
              : state.pendingFirstSend,
        });
    }
  };
  return {
    openNewChatDraft: async (projectId) =>
      openNewChatDraft(set, get, projectId),
    selectDraftHarness: (harnessId) => selectDraftHarness(set, get, harnessId),
    updateNewChatDraft: (patch) => {
      const draft = get().newChatDraft;
      if (!draft || draft.submission.status === "submitting") return;
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
        draft.submission.status === "submitting" ||
        state.selectedHarnessId !== draft.harnessId
      )
        return;
      if (!state.models.some((model) => model.id === modelId)) return;
      set({
        newChatDraft: reconcileDraftModel(
          { ...draft, modelId },
          draft.harnessId,
          state.models,
        ),
      });
    },
    sendNewChatDraft: async () => {
      const draft = get().newChatDraft;
      if (
        !draft ||
        draft.submission.status === "submitting" ||
        draft.submission.status === "unconfirmed" ||
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
        draft?.submission.status !== "unconfirmed" ||
        state.project?.id !== draft.projectId ||
        state.projectTransitioning ||
        state.pendingFirstSend
      )
        return;
      await submit(draft);
    },
  };
}
