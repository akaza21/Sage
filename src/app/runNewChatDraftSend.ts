import type { StoreApi } from "zustand";
import { boundedChatError } from "../domain/chatOwnership";
import { errorMessage } from "../domain/errors";
import { draftChatAccess } from "./chatAccessDrafts";
import { useChatEffortDraft } from "./chatEffortDraft";
import { effectiveDraftChoice } from "./newChatDraftNavigation";
import type { DraftCreation, NewChatDraft } from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";

export async function runNewChatDraftSend(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  attempt: NewChatDraft & {
    submission: { status: "submitting"; creation: DraftCreation };
  },
  submission: NonNullable<WorkbenchState["pendingFirstSend"]>,
) {
  let creation = attempt.submission.creation;
  // Each step reads the stored draft afresh; only this attempt may use it.
  const current = () => {
    const state = get();
    const draft = state.newChatDraft;
    if (
      state.project?.id !== attempt.projectId ||
      state.chatOwnershipEpoch !== submission.ownershipEpoch ||
      state.projectTransitioning ||
      draft?.submission !== attempt.submission
    )
      throw new Error("Project changed during submission. Retry to continue.");
    const choice = effectiveDraftChoice(draft, state);
    if (!choice)
      throw new Error("Choose an available harness and model before sending.");
    return { state, draft, choice };
  };
  try {
    const { state: initial } = current();
    if (initial.pendingFirstSend !== submission)
      throw new Error("Chat submission cancelled. Retry to continue.");

    if (creation.status !== "created") {
      const result = await (creation.status === "unconfirmed"
        ? creation.reconcile()
        : initial.createChat());
      // Record the durable result before validating this attempt's ownership.
      // A recovered project can invalidate the attempt, never its creation.
      switch (result.status) {
        case "confirmed":
          creation = {
            status: "created",
            chatId: result.chatId,
            editRevision: get().chatUiById[result.chatId]?.editRevision ?? 0,
          };
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
    current();
    const { chatId, editRevision } = creation;
    if (get().archivedChats.some((chat) => chat.id === chatId)) {
      if (!(await get().restoreChat(chatId)))
        throw new Error(
          "Could not reopen the created chat. Retry to continue.",
        );
    }
    if (get().activeTabId !== chatId)
      await get().activateTab(chatId, submission);
    const { state, draft, choice } = current();
    const owner = state.captureChatOwnership(chatId);
    if (!owner || state.activeTabId !== chatId)
      throw new Error("Could not open the created chat. Retry to continue.");
    const entry = state.chatUiById[chatId];
    if (entry?.composerText || entry?.attachedContext.length)
      throw new Error(
        "The new chat already has a message, so this prompt stays here. Clear that message to retry.",
      );
    // From here through clearing the draft, handoff is synchronous. No project
    // transition can erase the composer between its assignment and release.
    // Choices made in the chat since its creation are the user's and stay.
    if ((entry?.editRevision ?? 0) === editRevision) {
      state.setChatHarnessSelection(owner, {
        harnessId: choice.harnessId,
        modelId: choice.modelId,
      });
      if (choice.access)
        draftChatAccess(chatId, {
          bindingId: choice.bindingId,
          mode: choice.access,
        });
      if (choice.modelId && choice.effort)
        useChatEffortDraft.setState({
          draft: {
            chatId,
            bindingId: choice.bindingId,
            modelId: choice.modelId,
            effort: choice.effort,
          },
        });
    }
    state.setChatComposer(owner, draft.text);
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
