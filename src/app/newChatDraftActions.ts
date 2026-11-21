import type { StoreApi } from "zustand";
import {
  boundedChatError,
  validateChatComposerText,
} from "../domain/chatOwnership";
import { openNewChatDraft, selectDraftHarness } from "./newChatDraftNavigation";
import {
  liveDraftCreation,
  type NewChatDraft,
  type NewChatDraftSlice,
} from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { unsentChatIdsJson } from "./unsentChatIds";

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
  | "discardNewChatDraft"
> {
  const submit = async (draft: NewChatDraft) => {
    if (draft.submission.status === "submitting") return;
    const creation = liveDraftCreation(draft.submission, get());
    let attempt = {
      status: "submitting" as const,
      chatId:
        creation.status === "released" ? crypto.randomUUID() : creation.chatId,
      creation,
      reuse: false,
    };
    let submittingDraft = { ...draft, submission: attempt, error: null };
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
      const target =
        draft.targetProjectId === undefined
          ? draft.projectId
          : draft.targetProjectId;
      if (creation.status === "released" && target) {
        const chatId = await getWorkbenchPorts().chat.getReusableChat(
          target,
          JSON.parse(unsentChatIdsJson(get())),
        );
        const state = get();
        if (
          state.newChatDraft?.submission !== attempt ||
          state.pendingFirstSend !== submission
        )
          throw new Error("Chat submission cancelled. Retry to continue.");
        if (chatId) {
          attempt = { ...attempt, chatId, reuse: true };
          submittingDraft = {
            ...state.newChatDraft,
            submission: attempt,
            error: null,
          };
          set({ newChatDraft: submittingDraft });
        }
      }
      released = await send.runNewChatDraftSend(
        set,
        get,
        submittingDraft,
        submission,
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
      const model = state.models.find((model) => model.id === modelId);
      if (!model) return;
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
        draft.submission.status === "submitting" ||
        draft.submission.status === "unconfirmed" ||
        !draft.text.trim() ||
        get().projectTransitioning
      )
        return;
      await submit(draft);
    },
    discardNewChatDraft: () => {
      if (get().newChatDraft?.submission.status !== "submitting")
        set({ newChatDraft: null, agentCenter: "chat" });
    },
    reloadNewChatDraft: async () => {
      const state = get();
      const draft = state.newChatDraft;
      if (
        draft?.submission.status !== "unconfirmed" ||
        (state.project?.id ?? null) !== draft.projectId ||
        state.projectTransitioning ||
        state.pendingFirstSend
      )
        return;
      await submit(draft);
    },
  };
}
