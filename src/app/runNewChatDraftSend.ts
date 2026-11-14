import type { StoreApi } from "zustand";
import { boundedChatError } from "../domain/chatOwnership";
import { errorMessage } from "../domain/errors";
import { sessionAccessModeCandidates } from "../domain/sessionAccessMode";
import { draftChatAccess } from "./chatAccessDrafts";
import { useChatEffortDraft } from "./chatEffortDraft";
import type { DraftCreation, NewChatDraft } from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";

export async function runNewChatDraftSend(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  draft: NewChatDraft & {
    submission: { status: "submitting"; creation: DraftCreation };
  },
  submission: NonNullable<WorkbenchState["pendingFirstSend"]>,
  editRevision: number,
) {
  let creation = draft.submission.creation;
  const initial = get();
  const harness = initial.harnesses.find(
    (harness) => harness.id === draft.harnessId,
  );
  const model =
    initial.selectedHarnessId === draft.harnessId
      ? initial.models.find((model) => model.id === draft.modelId)
      : null;
  const requireProject = () => {
    const state = get();
    if (
      state.project?.id !== draft.projectId ||
      state.chatOwnershipEpoch !== submission.ownershipEpoch ||
      state.projectTransitioning ||
      state.newChatDraft?.id !== draft.id
    )
      throw new Error("Project changed during submission. Retry to continue.");
  };
  try {
    requireProject();
    if (initial.pendingFirstSend !== submission)
      throw new Error("Chat submission cancelled. Retry to continue.");
    if (
      !harness?.bindingId ||
      initial.harnessDiscoveryPhase !== "ready" ||
      harness.installState !== "available" ||
      initial.selectedHarnessId !== draft.harnessId ||
      (initial.models.length ? !model : draft.modelId !== null) ||
      (draft.access &&
        !sessionAccessModeCandidates(harness.accessModes, true).some(
          (mode) => mode.id === draft.access,
        )) ||
      (draft.effort && !model?.effortLevels?.includes(draft.effort))
    )
      throw new Error("Choose an available harness and model before sending.");

    if (creation.status !== "created") {
      const result = await (creation.status === "unconfirmed"
        ? creation.reconcile()
        : initial.createChat());
      // Record the durable result before validating this attempt's ownership.
      // A recovered project can invalidate the attempt, never its creation.
      switch (result.status) {
        case "confirmed":
          creation = { status: "created", chatId: result.chatId };
          break;
        case "unconfirmed":
          creation = {
            status: "unconfirmed",
            reconcile: result.reconcile ?? (() => Promise.resolve(result)),
          };
          throw new Error(
            get().error ??
              "Chat creation is unconfirmed. Reload chats before trying again.",
          );
        default:
          creation = { status: "released" };
          throw new Error(
            get().error ?? "Could not create the chat. Try again.",
          );
      }
    }
    requireProject();
    const chatId = creation.chatId;
    if (get().archivedChats.some((chat) => chat.id === chatId)) {
      if (!(await get().restoreChat(chatId)))
        throw new Error(
          "Could not reopen the created chat. Retry to continue.",
        );
    }
    if (get().activeTabId !== chatId)
      await get().activateTab(chatId, submission);
    requireProject();
    const state = get();
    const owner = state.captureChatOwnership(chatId);
    if (!owner || state.activeTabId !== chatId)
      throw new Error("Could not open the created chat. Retry to continue.");
    const entry = state.chatUiById[chatId];
    if (
      (entry?.editRevision ?? 0) !== editRevision ||
      entry?.composerText ||
      entry?.attachedContext.length
    )
      throw new Error(
        "The new chat was changed before this prompt reached it, so the prompt stays here. Retry once that chat's message is empty.",
      );
    // Catalog refreshes during creation reconcile the stored draft's choices.
    const { modelId, effort } = state.newChatDraft ?? draft;
    // From here through clearing the draft, handoff is synchronous. No project
    // transition can erase the composer between its assignment and release.
    state.setChatHarnessSelection(owner, {
      harnessId: draft.harnessId,
      modelId,
    });
    state.setChatComposer(owner, draft.text);
    if (draft.access)
      draftChatAccess(chatId, {
        bindingId: harness.bindingId,
        mode: draft.access,
      });
    if (modelId && effort)
      useChatEffortDraft.setState({
        draft: {
          chatId,
          bindingId: harness.bindingId,
          modelId,
          effort,
        },
      });
    set({
      newChatDraft: null,
      agentCenter: "chat",
      pendingFirstSend:
        state.pendingFirstSend === submission
          ? {
              ...submission,
              chatId,
              editRevision: get().chatUiById[chatId]?.editRevision ?? 0,
            }
          : null,
    });
    return { submission: creation, error: "" };
  } catch (error) {
    return {
      submission: creation,
      error: boundedChatError(errorMessage(error)),
    };
  }
}
