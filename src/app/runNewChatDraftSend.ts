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
    if (owns()) set({ newChatDraft: { ...draft, creating: false } });
    return;
  }
  const fail = (error: string) => {
    if (owns())
      set({
        newChatDraft: {
          ...draft,
          creating: false,
          error: boundedChatError(error),
        },
        pendingFirstSend: null,
      });
  };
  if (
    initial.project?.id !== draft.projectId ||
    !harness?.bindingId ||
    initial.harnessDiscoveryPhase !== "ready" ||
    harness.installState !== "available" ||
    !model ||
    (draft.access &&
      !sessionAccessModeCandidates(harness.accessModes, true).some(
        (mode) => mode.id === draft.access,
      )) ||
    (draft.effort && !model.effortLevels?.includes(draft.effort))
  ) {
    fail("Choose an available harness and model before sending.");
    return;
  }
  const result = await initial.createChat();
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
        error: boundedChatError(get().error ?? "Chat creation is unconfirmed."),
        unconfirmedCreation: {
          priorChatIds: initial.tabs
            .filter((tab) => tab.kind === "chat")
            .map((tab) => tab.id),
        },
      },
      pendingFirstSend: null,
    });
    reconcileNewChatDraft(set, get);
    return;
  }
  if (result.status === "confirmed")
    adoptDraft(
      set,
      get,
      draft,
      result.chatId,
      get().pendingFirstSend === submission,
    );
}

export function reconcileNewChatDraft(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
) {
  const state = get();
  const draft = state.newChatDraft;
  if (!draft?.unconfirmedCreation || state.project?.id !== draft.projectId)
    return;
  const priorIds = new Set(draft.unconfirmedCreation.priorChatIds);
  const created = state.tabs.filter(
    (tab) => tab.kind === "chat" && !priorIds.has(tab.id),
  );
  if (created.length === 1) adoptDraft(set, get, draft, created[0].id, false);
}

function adoptDraft(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  draft: NewChatDraft,
  chatId: string,
  dispatch: boolean,
) {
  const state = get();
  const owner = state.captureChatOwnership(chatId);
  if (!owner) return;
  const harness = state.harnesses.find(
    (harness) => harness.id === draft.harnessId,
  );
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
  set({
    newChatDraft: null,
    agentCenter: "chat",
    pendingFirstSend: dispatch
      ? {
          projectId: draft.projectId,
          chatId,
          ownershipEpoch: state.chatOwnershipEpoch,
        }
      : null,
  });
}
