import type { StoreApi } from "zustand";
import { boundedChatError } from "../domain/chatOwnership";
import { currentProjectLayout } from "./projectProjection";
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
    submission: {
      status: "submitting";
      chatId: string;
      creation: DraftCreation;
    };
  },
  submission: NonNullable<WorkbenchState["pendingFirstSend"]>,
) {
  let creation = attempt.submission.creation;
  let opening = false;
  // Each step reads the stored draft afresh; only this attempt may use it.
  const current = () => {
    const state = get();
    const draft = state.newChatDraft;
    if (
      (state.project?.id ?? null) !== draft?.projectId ||
      state.chatOwnershipEpoch !== submission.ownershipEpoch ||
      state.projectTransitioning ||
      draft?.submission !== attempt.submission
    )
      throw new Error("Project changed during submission. Retry to continue.");
    return { state, draft };
  };
  try {
    const { state: initial, draft: sent } = current();
    // Creating and opening the chat can reselect the harness, so the choice is
    // fixed against the catalog the user sent from.
    const choice = effectiveDraftChoice(sent, initial);
    if (!choice)
      throw new Error("Choose an available harness and model before sending.");
    if (initial.pendingFirstSend !== submission)
      throw new Error("Chat submission cancelled. Retry to continue.");

    if (creation.status !== "created") {
      const result = await (creation.status === "unconfirmed"
        ? creation.reconcile()
        : initial.createChat(
            attempt.targetProjectId,
            attempt.submission.chatId,
          ));
      // Record the durable result before validating this attempt's ownership.
      // A recovered project can invalidate the attempt, never its creation.
      switch (result.status) {
        case "confirmed":
          creation = { status: "created", chatId: result.chatId };
          break;
        case "unconfirmed":
          creation = {
            status: "unconfirmed",
            chatId: attempt.submission.chatId,
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
    if (creation.status === "created") {
      const state = get();
      const createdChatId = creation.chatId;
      const target =
        attempt.targetProjectId === undefined
          ? attempt.projectId
          : attempt.targetProjectId;
      const actual = state.project?.isInternal ? null : state.project?.id;
      if (target !== actual) {
        current();
        if (get().archivedChats.some((chat) => chat.id === createdChatId)) {
          if (!(await get().restoreChat(createdChatId)))
            throw new Error("Could not reopen the created chat.");
        }
        const source = currentProjectLayout(get());
        if (
          !source?.tabs.some(
            (tab) => tab.id === createdChatId && tab.kind === "chat",
          )
        )
          throw new Error("Created chat is no longer attached.");
        await get().setChatProject(target, {
          ...source,
          activeTabId: createdChatId,
        });
        if ((get().project?.isInternal ? null : get().project?.id) !== target)
          throw new Error(get().error ?? "Could not choose the Chat project.");
      }
    }
    const { state: scoped } = current();
    const accountModels = initial.harnesses.find(
      (harness) => harness.id === choice.harnessId,
    )?.accountModelDiscovery;
    if (accountModels && scoped.project?.id !== initial.project?.id) {
      // Account discovery is scoped to the native cwd. Confirm the chosen
      // model for the newly created workspace before handing off the prompt.
      await scoped.loadHarnessModelsForProjection(
        choice.harnessId,
        choice.modelId,
        scoped.projectGeneration,
        scoped.project!.id,
      );
      const { state: discovered } = current();
      if (
        discovered.harnessDiscoveryPhase !== "ready" ||
        (choice.modelId &&
          !discovered.models.some((model) => model.id === choice.modelId))
      ) {
        throw new Error(
          "Choose an available model for this chat's project before sending.",
        );
      }
    }
    current();
    const { chatId } = creation;
    if (get().pendingFirstSend !== submission)
      throw new Error("Chat submission cancelled. Retry to continue.");
    opening = true;
    if (get().archivedChats.some((chat) => chat.id === chatId)) {
      if (!(await get().restoreChat(chatId)))
        throw new Error(
          "Could not reopen the created chat. Retry to continue.",
        );
    }
    if (get().activeTabId !== chatId)
      await get().activateTab(chatId, submission);
    const { state, draft } = current();
    const owner = state.captureChatOwnership(chatId, draft.id);
    if (!owner || state.activeTabId !== chatId)
      throw new Error("Could not open the created chat. Retry to continue.");
    // From here through clearing the draft, handoff is synchronous. No project
    // transition can erase the composer between its assignment and release.
    state.setChatHarnessSelection(owner, {
      harnessId: choice.harnessId,
      modelId: choice.modelId,
    });
    if (choice.bindingId && choice.access)
      draftChatAccess(chatId, {
        bindingId: choice.bindingId,
        mode: choice.access,
      });
    if (choice.bindingId && choice.modelId && choice.effort)
      useChatEffortDraft.setState({
        draft: {
          chatId,
          bindingId: choice.bindingId,
          modelId: choice.modelId,
          effort: choice.effort,
        },
      });
    state.setChatComposer(owner, draft.text);
    set({
      newChatDraft: null,
      agentCenter: "chat",
      pendingFirstSend:
        state.pendingFirstSend === submission
          ? {
              ...submission,
              projectId: owner.projectId,
              chatId,
              text: draft.text,
              images: draft.images,
            }
          : null,
    });
    return { submission: creation, error: "" };
  } catch (error) {
    // Opening the chat moved the center to it; unless the user navigated
    // since, a failed handoff returns to the draft that still holds the prompt.
    if (
      opening &&
      get().pendingFirstSend === submission &&
      get().newChatDraft?.submission === attempt.submission
    )
      set({ agentCenter: "draft" });
    return {
      submission: creation,
      error: boundedChatError(errorMessage(error)),
    };
  }
}
