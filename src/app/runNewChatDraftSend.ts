import type { StoreApi } from "zustand";
import { boundedChatError } from "../domain/chatOwnership";
import { sessionAccessModeCandidates } from "../domain/sessionAccessMode";
import { draftChatAccess } from "./chatAccessDrafts";
import { useChatEffortDraft } from "./chatEffortDraft";
import type { NewChatDraft } from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";

export async function runNewChatDraftSend(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  draft: NewChatDraft,
  submission: NonNullable<WorkbenchState["pendingFirstSend"]>,
) {
  const initial = get();
  const epoch = submission.ownershipEpoch;
  const harness = initial.harnesses.find(
    (harness) => harness.id === draft.harnessId,
  );
  const model =
    initial.selectedHarnessId === draft.harnessId
      ? initial.models.find((model) => model.id === draft.modelId)
      : null;
  const owns = () =>
    get().project?.id === draft.projectId &&
    get().chatOwnershipEpoch === epoch &&
    get().newChatDraft?.creating;
  if (initial.pendingFirstSend !== submission) {
    if (owns())
      set({
        newChatDraft: { ...draft, creating: !!draft.reconcileCreation },
      });
    return;
  }
  const fail = (error: string) => {
    if (owns())
      set({
        newChatDraft: {
          ...draft,
          creating: false,
          error: boundedChatError(error),
          reconcileCreation: undefined,
        },
        pendingFirstSend: null,
      });
  };
  if (
    !draft.reconcileCreation &&
    (initial.project?.id !== draft.projectId ||
      !harness?.bindingId ||
      initial.harnessDiscoveryPhase !== "ready" ||
      harness.installState !== "available" ||
      !model ||
      (draft.access &&
        !sessionAccessModeCandidates(harness.accessModes, true).some(
          (mode) => mode.id === draft.access,
        )) ||
      (draft.effort && !model.effortLevels?.includes(draft.effort)))
  ) {
    fail("Choose an available harness and model before sending.");
    return;
  }
  const result = await (draft.reconcileCreation
    ? draft.reconcileCreation()
    : initial.createChat());
  if (!owns()) return;
  if (result.status === "not-created") {
    fail(get().error ?? "Could not create the chat. Try again.");
    return;
  }
  if (result.status === "unconfirmed") {
    set({
      newChatDraft: {
        ...draft,
        creating: true,
        error: boundedChatError(
          get().error ??
            "Chat creation is unconfirmed. Reload chats before trying again.",
        ),
        reconcileCreation: result.reconcile,
      },
      pendingFirstSend: null,
    });
    return;
  }
  if (result.status !== "confirmed") return;
  const chatId = result.chatId;
  const state = get();
  const owner = state.captureChatOwnership(chatId);
  if (!owner) return;
  state.setChatHarnessSelection(owner, {
    harnessId: draft.harnessId,
    modelId: draft.modelId,
  });
  state.setChatComposer(owner, draft.text);
  if (harness?.bindingId && draft.access)
    draftChatAccess(chatId, {
      bindingId: harness.bindingId,
      mode: draft.access,
    });
  if (harness?.bindingId && draft.modelId && draft.effort)
    useChatEffortDraft.setState({
      draft: {
        chatId,
        bindingId: harness.bindingId,
        modelId: draft.modelId,
        effort: draft.effort,
      },
    });
  const dispatch =
    state.pendingFirstSend === submission && state.activeTabId === chatId;
  set({
    newChatDraft: null,
    agentCenter: "chat",
    pendingFirstSend: dispatch
      ? {
          projectId: draft.projectId,
          chatId,
          ownershipEpoch: epoch,
        }
      : null,
  });
  if (state.activeTabId !== chatId) await state.activateTab(chatId);
}
